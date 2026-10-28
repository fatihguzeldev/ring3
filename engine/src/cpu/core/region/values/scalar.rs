use super::super::RegionInstruction;
use super::{BlockId, EffectId, FrameStateId, Region, RegionPlan, ValueId};
use crate::cpu::core::flags::{self, FlagKind};
use crate::cpu::x86::ir::{AddressSize, EffectiveAddress};
use std::collections::{BTreeMap, VecDeque};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cpu::core) struct FlagRecipe {
    pub kind: FlagKind,
    pub width: u32,
}

impl FlagRecipe {
    // INC/DEC's carry is already materialized before installing their tuple.
    // The other requested bits still belong to this literal-width writer.
    pub fn written(self) -> u32 {
        match self.kind {
            FlagKind::None => 0,
            FlagKind::Inc | FlagKind::Dec => flags::ARITHMETIC & !flags::CF,
            _ => flags::ARITHMETIC,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cpu::core) enum FlagUpdate {
    Preserve,
    Recipe(FlagRecipe),
    Materialized,
    Opaque,
}

impl FlagUpdate {
    fn apply(self, previous: Option<FlagRecipe>) -> Option<FlagRecipe> {
        match self {
            Self::Preserve => previous,
            Self::Recipe(recipe) => Some(recipe),
            Self::Materialized => Some(FlagRecipe {
                kind: FlagKind::None,
                width: 32,
            }),
            Self::Opaque => None,
        }
    }
}

fn needed_update(instruction: &RegionInstruction, live: u32, update: FlagUpdate) -> FlagUpdate {
    if matches!(update, FlagUpdate::Recipe(_))
        && instruction
            .operation
            .and_then(crate::cpu::core::effects::analyze)
            .is_some_and(|effect| effect.write & live == 0)
    {
        FlagUpdate::Preserve
    } else {
        update
    }
}

// Produced by the same operand normalization that emits the instruction. An
// unsupported effect is deliberately opaque, rather than a guessed no-write.
#[derive(Clone, Copy, Debug)]
pub(in crate::cpu::core) struct ScalarEffect {
    pub flags: FlagUpdate,
    pub read: Option<(EffectiveAddress, u32)>,
    pub gpr_writes: u8,
    pub memory_barrier: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cpu::core) struct ReadAvailability {
    pub address: EffectiveAddress,
    pub width: u32,
    pub slot: u32,
    pub first: bool,
}

// Register generations distinguish actual definitions even where speculative
// constant propagation interns two unequal runtime values as one ValueId.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct ReadKey {
    authority: u32,
    memory: EffectId,
    base: Option<(usize, ValueId, u32)>,
    index: Option<(usize, ValueId, u32)>,
    scale: u8,
    displacement: u32,
    segment: usize,
    word_address: bool,
    width: u32,
}

pub(in crate::cpu::core) struct ScalarPlan {
    recipes: Vec<Option<FlagRecipe>>,
    guarded: Vec<bool>,
    reads: Vec<Vec<Option<ReadAvailability>>>,
    cache_count: u32,
}

impl ScalarPlan {
    pub fn analyze(graph: &Region, values: &RegionPlan) -> Self {
        let mut result = Self {
            recipes: vec![None; values.frames().len()],
            guarded: vec![false; values.frames().len()],
            reads: Vec::with_capacity(graph.blocks.len()),
            cache_count: 0,
        };
        let mut exits = Vec::with_capacity(graph.blocks.len());
        let mut passthrough = Vec::with_capacity(graph.blocks.len());
        for (index, block) in graph.blocks.iter().enumerate() {
            let id = BlockId(index);
            let mut recipe = None;
            let mut passes_entry = true;
            let mut generations = [0; 8];
            let mut authority = 0;
            let mut reads = vec![None; block.instructions.len()];
            let mut groups = BTreeMap::<ReadKey, Vec<(u32, EffectiveAddress)>>::new();
            for instruction in &block.instructions {
                let instruction_plan = values.instruction(id, instruction.prefix);
                result.recipes[instruction_plan.before.0] = recipe;
                let effect = instruction
                    .operation
                    .filter(|_| instruction.gate.is_none())
                    .map(crate::cpu::core::emitter::scalar_effect)
                    .unwrap_or(ScalarEffect {
                        flags: FlagUpdate::Opaque,
                        read: None,
                        gpr_writes: u8::MAX,
                        memory_barrier: true,
                    });
                if effect.memory_barrier {
                    authority += 1;
                }
                if let Some((address, width)) = effect.read {
                    let frame = values.frame(instruction_plan.before);
                    let input = |register: crate::cpu::x86::Register32| {
                        let index = register.index();
                        (index, frame.gpr[index], generations[index])
                    };
                    let key = ReadKey {
                        authority,
                        memory: frame.memory_effect,
                        base: address.base.map(input),
                        index: address.index.map(input),
                        scale: address.scale,
                        displacement: address.displacement,
                        segment: address.resolved_segment().index(),
                        word_address: address.address_size == AddressSize::Bits16,
                        width,
                    };
                    groups
                        .entry(key)
                        .or_default()
                        .push((instruction.prefix, address));
                }
                for (register, generation) in generations.iter_mut().enumerate() {
                    if effect.gpr_writes & (1 << register) != 0 {
                        *generation += 1;
                    }
                }
                let update = needed_update(
                    instruction,
                    block.flags.after[instruction.prefix as usize],
                    effect.flags,
                );
                passes_entry &= update == FlagUpdate::Preserve;
                recipe = update.apply(recipe);
                result.recipes[instruction_plan.after.0] = recipe;
            }
            exits.push(recipe);
            passthrough.push(passes_entry);
            for (key, members) in groups {
                if members.len() < 2 {
                    continue;
                }
                let slot = result.cache_count;
                result.cache_count += 1;
                for (member, (prefix, address)) in members.into_iter().enumerate() {
                    reads[prefix as usize] = Some(ReadAvailability {
                        address,
                        width: key.width,
                        slot,
                        first: member == 0,
                    });
                }
            }
            result.reads.push(reads);
        }
        let mut entries = vec![None; graph.blocks.len()];
        let mut pending: VecDeque<_> = (0..graph.blocks.len()).collect();
        while let Some(index) = pending.pop_front() {
            let block = &graph.blocks[index];
            let incoming = block
                .predecessors
                .first()
                .and_then(|id| exits[id.0])
                .filter(|recipe| {
                    block
                        .predecessors
                        .iter()
                        .all(|id| exits[id.0] == Some(*recipe))
                });
            entries[index] = incoming;
            if passthrough[index] && exits[index] != incoming {
                exits[index] = incoming;
                pending.extend(block.successors.iter().map(|id| id.0));
            }
        }
        for (index, block) in graph.blocks.iter().enumerate() {
            let Some(recipe) = entries[index] else {
                continue;
            };
            // public and secondary entries still validate the actual lazy tuple.
            for instruction in &block.instructions {
                let plan = values.instruction(BlockId(index), instruction.prefix);
                result.recipes[plan.before.0] = Some(recipe);
                result.guarded[plan.before.0] = true;
                let update = instruction
                    .operation
                    .filter(|_| instruction.gate.is_none())
                    .map(crate::cpu::core::emitter::scalar_effect)
                    .map_or(FlagUpdate::Opaque, |effect| effect.flags);
                if needed_update(
                    instruction,
                    block.flags.after[instruction.prefix as usize],
                    update,
                ) != FlagUpdate::Preserve
                {
                    break;
                }
                result.recipes[plan.after.0] = Some(recipe);
                result.guarded[plan.after.0] = true;
            }
        }
        result
    }

    pub fn recipe(&self, frame: FrameStateId) -> Option<FlagRecipe> {
        self.recipes[frame.0]
    }

    pub fn recipe_requires_guard(&self, frame: FrameStateId) -> bool {
        self.guarded[frame.0]
    }

    pub fn read(
        &self,
        block: BlockId,
        prefix: u32,
        address: EffectiveAddress,
        width: u32,
    ) -> Option<ReadAvailability> {
        self.reads[block.0]
            .get(prefix as usize)
            .copied()
            .flatten()
            .filter(|read| read.address == address && read.width == width)
    }

    pub fn cache_count(&self) -> u32 {
        self.cache_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cpu::core::program;
    use crate::cpu::x86::{Register32, X87Operation, ir::*};

    fn plans(operations: &[Operation]) -> (RegionPlan, ScalarPlan) {
        let block = program::Block {
            entry: 0x1000,
            instructions: operations
                .iter()
                .copied()
                .enumerate()
                .map(|(index, operation)| program::Instruction {
                    pc: 0x1000 + index as u32,
                    next: 0x1001 + index as u32,
                    operation: Some(operation),
                    gate: None,
                })
                .collect(),
        };
        let region = Region::from_blocks(&[block], 0x1000).unwrap();
        let values = RegionPlan::analyze(&region);
        let scalar = ScalarPlan::analyze(&region, &values);
        (values, scalar)
    }

    fn load(register: Register32, address: EffectiveAddress) -> Operation {
        Operation::Move {
            destination: Location32::Register(register),
            source: Value32::Memory(address),
        }
    }

    fn block(pc: u32, operations: &[Operation]) -> program::Block {
        program::Block {
            entry: pc,
            instructions: operations
                .iter()
                .copied()
                .enumerate()
                .map(|(index, operation)| program::Instruction {
                    pc: pc + index as u32,
                    next: pc + index as u32 + 1,
                    operation: Some(operation),
                    gate: None,
                })
                .collect(),
        }
    }

    fn jump(pc: u32) -> Operation {
        Operation::Jump {
            target: BranchTarget::Direct(crate::memory::GuestAddress(pc)),
        }
    }

    fn writer(kind: BinaryKind) -> Operation {
        Operation::Binary {
            kind,
            destination: Location32::Register(Register32::Eax),
            source: Value32::Immediate(1),
        }
    }

    #[test]
    fn predecessor_recipe_reaches_preserving_blocks_and_observed_return() {
        let blocks = [
            block(0x1000, &[writer(BinaryKind::Add), jump(0x2000)]),
            block(0x2000, &[Operation::Return { stack_adjust: 0 }]),
            block(
                0x3000,
                &[
                    Operation::Unary {
                        kind: UnaryKind::Dec,
                        destination: Location32::Register(Register32::Ecx),
                    },
                    Operation::Nop,
                ],
            ),
        ];
        let region = Region::from_observed(
            &blocks,
            0x1000,
            &[
                (0x1000, 0x1001),
                (0x1001, 0x2000),
                (0x2000, 0x3000),
                (0x3000, 0x3001),
            ],
        )
        .unwrap();
        let values = RegionPlan::analyze(&region);
        let scalar = ScalarPlan::analyze(&region, &values);
        for id in [BlockId(1), BlockId(2)] {
            let before = values.instruction(id, 0).before;
            let recipe = scalar.recipe(before).unwrap();
            assert_eq!((recipe.kind, recipe.width), (FlagKind::Add, 32));
            assert!(scalar.recipe_requires_guard(before));
        }
        assert!(!scalar.recipe_requires_guard(values.instruction(BlockId(2), 0).after));
    }

    #[test]
    fn predecessor_recipe_join_requires_matching_known_writers() {
        for (right, expected) in [
            (writer(BinaryKind::Add), Some(FlagKind::Add)),
            (writer(BinaryKind::Sub), None),
            (
                Operation::ArithmeticWord {
                    kind: WordArithmeticKind::Add,
                    destination: Register32::Eax,
                    source: WordValue::Immediate(1),
                },
                None,
            ),
            (Operation::Nop, None),
        ] {
            let blocks = [
                block(0x1000, &[writer(BinaryKind::Add), jump(0x3000)]),
                block(0x2000, &[right, jump(0x3000)]),
                block(0x3000, &[Operation::Nop]),
            ];
            let region = Region::from_blocks(&blocks, 0x1000).unwrap();
            let values = RegionPlan::analyze(&region);
            let scalar = ScalarPlan::analyze(&region, &values);
            assert_eq!(
                scalar
                    .recipe(values.instruction(BlockId(2), 0).before)
                    .map(|recipe| recipe.kind),
                expected
            );
        }
    }

    #[test]
    fn predecessor_recipe_does_not_cross_opaque_instruction_or_unknown_cycle() {
        let mut barrier = block(0x2000, &[Operation::Nop, jump(0x3000)]);
        barrier.instructions[0].operation = None;
        let blocks = [
            block(0x1000, &[writer(BinaryKind::Add), jump(0x2000)]),
            barrier,
            block(0x3000, &[Operation::Nop]),
            block(0x4000, &[jump(0x5000)]),
            block(0x5000, &[jump(0x4000)]),
        ];
        let region = Region::from_blocks(&blocks, 0x1000).unwrap();
        let values = RegionPlan::analyze(&region);
        let scalar = ScalarPlan::analyze(&region, &values);
        for id in [BlockId(2), BlockId(3), BlockId(4)] {
            assert_eq!(scalar.recipe(values.instruction(id, 0).before), None);
        }
    }

    #[test]
    fn predecessor_recipe_materialization_replaces_the_inherited_writer() {
        let blocks = [
            block(0x1000, &[writer(BinaryKind::Add), jump(0x2000)]),
            block(
                0x2000,
                &[
                    Operation::Carry {
                        kind: CarryKind::Clear,
                    },
                    jump(0x3000),
                ],
            ),
            block(0x3000, &[Operation::Nop]),
        ];
        let region = Region::from_blocks(&blocks, 0x1000).unwrap();
        let values = RegionPlan::analyze(&region);
        let scalar = ScalarPlan::analyze(&region, &values);
        let recipe = scalar
            .recipe(values.instruction(BlockId(2), 0).before)
            .unwrap();
        assert_eq!(
            (recipe.kind, recipe.width, recipe.written()),
            (FlagKind::None, 32, 0)
        );
    }

    #[test]
    fn successful_writer_has_literal_width_and_independent_preserved_carry() {
        let (values, scalar) = plans(&[
            Operation::Binary {
                kind: BinaryKind::Add,
                destination: Location32::Register(Register32::Eax),
                source: Value32::Immediate(1),
            },
            Operation::UnaryWord {
                kind: UnaryKind::Inc,
                destination: Register32::Ecx,
            },
            Operation::Nop,
        ]);
        assert_eq!(
            scalar.recipe(values.instruction(BlockId(0), 0).before),
            None
        );
        let add = scalar
            .recipe(values.instruction(BlockId(0), 1).before)
            .unwrap();
        assert_eq!(
            (add.kind, add.width, add.written()),
            (FlagKind::Add, 32, flags::ARITHMETIC)
        );
        let inc = scalar
            .recipe(values.instruction(BlockId(0), 2).before)
            .unwrap();
        assert_eq!((inc.kind, inc.width), (FlagKind::Inc, 16));
        assert_eq!(inc.written() & flags::CF, 0);
    }

    #[test]
    fn repeated_read_requires_actual_first_read_and_matching_definition() {
        let address = EffectiveAddress {
            base: Some(Register32::Ebx),
            ..Default::default()
        };
        let (_, scalar) = plans(&[
            load(Register32::Eax, address),
            Operation::Nop,
            load(Register32::Edx, address),
        ]);
        assert_eq!(scalar.cache_count(), 1);
        let first = scalar.read(BlockId(0), 0, address, 4).unwrap();
        let second = scalar.read(BlockId(0), 2, address, 4).unwrap();
        assert!(first.first);
        assert!(!second.first);
        assert_eq!(first.slot, second.slot);
        assert!(scalar.read(BlockId(0), 2, address, 2).is_none());
    }

    #[test]
    fn omitted_noncarry_writer_keeps_the_actual_previous_pending_recipe() {
        let (values, scalar) = plans(&[
            Operation::Binary {
                kind: BinaryKind::Add,
                destination: Location32::Register(Register32::Eax),
                source: Value32::Immediate(1),
            },
            Operation::Unary {
                kind: UnaryKind::Inc,
                destination: Location32::Register(Register32::Ecx),
            },
            Operation::SetByte {
                condition: Condition::Below,
                destination: ByteRegister::Dl,
            },
            Operation::Binary {
                kind: BinaryKind::Cmp,
                destination: Location32::Register(Register32::Ebx),
                source: Value32::Immediate(0),
            },
        ]);
        let recipe = scalar
            .recipe(values.instruction(BlockId(0), 2).before)
            .unwrap();
        assert_eq!(recipe.kind, FlagKind::Add);
    }

    #[test]
    fn speculative_constant_identity_does_not_reuse_after_register_definition() {
        let address = EffectiveAddress {
            base: Some(Register32::Ebx),
            ..Default::default()
        };
        let (values, scalar) = plans(&[
            Operation::Move {
                destination: Location32::Register(Register32::Ebx),
                source: Value32::Immediate(7),
            },
            load(Register32::Eax, address),
            Operation::Binary {
                kind: BinaryKind::Add,
                destination: Location32::Register(Register32::Ebx),
                source: Value32::Immediate(0),
            },
            load(Register32::Edx, address),
        ]);
        let first = values.frame(values.instruction(BlockId(0), 1).before).gpr[3];
        let second = values.frame(values.instruction(BlockId(0), 3).before).gpr[3];
        assert_eq!(values.constant(first), values.constant(second));
        assert_eq!(scalar.cache_count(), 0);
    }

    #[test]
    fn stores_and_numeric_helpers_kill_memory_read_availability() {
        let address = EffectiveAddress {
            base: Some(Register32::Ebx),
            ..Default::default()
        };
        for barrier in [
            Operation::Move {
                destination: Location32::Memory(address),
                source: Value32::Immediate(9),
            },
            Operation::X87 {
                operation: X87Operation::StatusToAx,
                address: None,
                waiting: false,
                opcode: 0x123,
            },
        ] {
            let (_, scalar) = plans(&[
                load(Register32::Eax, address),
                barrier,
                load(Register32::Edx, address),
            ]);
            assert_eq!(scalar.cache_count(), 0);
        }
    }

    #[test]
    fn rmw_never_advertises_a_cached_read_instead_of_prepared_geometry() {
        let address = EffectiveAddress {
            base: Some(Register32::Ebx),
            ..Default::default()
        };
        let (_, scalar) = plans(&[
            load(Register32::Eax, address),
            Operation::Binary {
                kind: BinaryKind::Add,
                destination: Location32::Memory(address),
                source: Value32::Immediate(1),
            },
            load(Register32::Edx, address),
        ]);
        assert_eq!(scalar.cache_count(), 0);
    }
}
