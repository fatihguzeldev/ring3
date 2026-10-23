use super::*;
use crate::cpu::x86::ir::{BranchTarget, Location32, Operation, Value32};

pub(super) fn analyze(region: &Region) -> RegionPlan {
    let facts = flow::entries(region);
    let mut plan = RegionPlan {
        blocks: Vec::with_capacity(region.blocks.len()),
        values: Vec::new(),
        proofs: Vec::new(),
        effects: Vec::new(),
        frames: Vec::new(),
        storage: Vec::new(),
        shapes: Vec::new(),
        frame_shapes: Vec::new(),
        frame_proofs: Vec::new(),
        frame_outputs: Vec::new(),
        shape_index: Default::default(),
        constants: Default::default(),
        constant_facts: Vec::new(),
    };
    for (index, block) in region.blocks.iter().enumerate() {
        let id = BlockId(index);
        let (frame, guards) =
            super::fp::entry(&mut plan, id, ResumePc::Exact(block.entry), facts[index]);
        let entry = plan.allocate_frame(frame);
        plan.frame_proofs[entry.0] = guards.clone();
        plan.blocks.push(BlockPlan {
            entry,
            exit: entry,
            instructions: Vec::with_capacity(block.instructions.len()),
            contract: EntryContract {
                frame: entry,
                external: true,
                guards,
                gpr_constants: facts[index].gpr,
            },
            edges: Vec::new(),
        });
    }
    for (index, block) in region.blocks.iter().enumerate() {
        let id = BlockId(index);
        let mut current = plan.block(id).entry;
        let mut available = plan.block(id).contract.guards.clone();
        for instruction in &block.instructions {
            let point = ProgramPoint {
                block: id,
                prefix: instruction.prefix,
            };
            let before = current;
            let mut frame = plan.frame(before).clone();
            let mut shape = plan.shape(before).clone();
            let mut inputs = Vec::new();
            let mut outputs = Vec::new();
            let mut proofs = available.clone();
            let mut effects = Vec::new();
            let address = instruction
                .operation
                .and_then(operation_address)
                .map(|address| AddressPlan {
                    address,
                    base: address.base.map(|r| frame.gpr[r.index()]),
                    index: address.index.map(|r| frame.gpr[r.index()]),
                    memory_epoch: frame.memory_effect,
                });
            if let Some(address) = &address {
                inputs.extend(address.base);
                inputs.extend(address.index);
            }
            super::gpr::lower(
                &mut plan,
                &mut frame,
                instruction.operation,
                point,
                &mut inputs,
                &mut outputs,
            );
            if instruction
                .operation
                .and_then(crate::cpu::core::effects::analyze)
                .is_none_or(|e| e.write != 0)
            {
                frame.flags_effect =
                    plan.allocate_effect(EffectKind::Flags, Some(point), vec![frame.flags_effect]);
                effects.push(frame.flags_effect);
            }
            let helper = instruction
                .operation
                .is_none_or(super::fp::canonical_numeric);
            if instruction
                .operation
                .is_none_or(|op| crate::cpu::core::effects::memory_barrier(op).1)
                || helper
            {
                frame.memory_effect = plan.allocate_effect(
                    EffectKind::Memory,
                    Some(point),
                    vec![frame.memory_effect],
                );
                effects.push(frame.memory_effect);
            }
            let fp = instruction.operation.and_then(super::fp::operation).map(
                |(operation, address, opcode)| {
                    super::fp::lower(
                        &mut plan,
                        &mut frame,
                        &mut shape,
                        operation,
                        address,
                        opcode,
                        point,
                        &mut available,
                        &mut proofs,
                        &mut inputs,
                        &mut outputs,
                        &mut effects,
                    )
                },
            );
            if fp.is_none() && helper {
                super::fp::kill(&mut plan, &mut frame, &mut shape, point);
                available.clear();
            }
            let after = instruction.exits.after.unwrap_or(instruction.exits.before);
            frame.pc = after.pc;
            frame.retired = after.retired;
            shape.tags = std::array::from_fn(|i| plan.tag_recipe(frame.fp.tags[i], shape.cells[i]));
            shape.top_delta = frame.fp.top_delta;
            current = plan.allocate_frame_with_shape(frame, shape);
            plan.frame_outputs[current.0] = fp
                .as_ref()
                .and_then(|step| step.outputs.first())
                .and_then(|payload| {
                    payload
                        .values()
                        .first()
                        .and_then(|&value| plan.storage(value))
                })
                .map(|storage| storage.cell);
            let live = plan
                .frame(current)
                .fp
                .payload
                .iter()
                .flat_map(|payload| payload.values())
                .collect::<Vec<_>>();
            available.retain(|&id| {
                let proof = plan.proof(id);
                proof.epoch == plan.frame(current).fp.control_epoch
                    && proof.value.is_none_or(|value| live.contains(&value))
                    && proof.kind != ProofKind::Empty
            });
            plan.frame_proofs[current.0] = available.clone();
            plan.blocks[index].instructions.push(InstructionPlan {
                before,
                after: current,
                inputs,
                outputs,
                proofs,
                fp,
                address,
                effects,
                helper_after: None,
            });
        }
        plan.blocks[index].exit = current;
    }
    connect(&mut plan, region);
    plan
}

pub(super) fn definition(
    plan: &mut RegionPlan,
    point: ProgramPoint,
    output: u8,
    ty: ValueType,
) -> ValueId {
    plan.allocate_value(ty, ValueKind::Instruction { point, output })
}

fn connect(plan: &mut RegionPlan, region: &Region) {
    // Phi inputs describe internal edges. They never erase the separately guarded
    // external-entry contract: selecting any advertised block still imports actual state.
    for (index, block) in region.blocks.iter().enumerate() {
        if block.predecessors.is_empty() {
            continue;
        }
        let entry = plan.frame(plan.blocks[index].entry).clone();
        for register in 0..8 {
            let inputs = block
                .predecessors
                .iter()
                .map(|&predecessor| {
                    (
                        predecessor,
                        plan.frame(plan.blocks[predecessor.0].exit).gpr[register],
                    )
                })
                .collect();
            plan.values[entry.gpr[register].0].kind = ValueKind::Phi {
                block: BlockId(index),
                inputs,
            };
        }
        for kind in [
            EffectKind::Flags,
            EffectKind::Memory,
            EffectKind::Control,
            EffectKind::Status,
            EffectKind::Metadata,
        ] {
            let select = |frame: &FrameState| match kind {
                EffectKind::Flags => frame.flags_effect,
                EffectKind::Memory => frame.memory_effect,
                EffectKind::Control => frame.fp.control_epoch,
                EffectKind::Status => frame.fp.status_effect,
                EffectKind::Metadata => frame.fp.metadata_effect,
            };
            let inputs = block
                .predecessors
                .iter()
                .map(|predecessor| select(plan.frame(plan.blocks[predecessor.0].exit)))
                .collect();
            plan.effects[select(&entry).0].inputs = inputs;
        }
    }
    for (index, block) in region.blocks.iter().enumerate() {
        let source = plan.blocks[index].exit;
        let from = plan.frame(source).clone();
        for &target in &block.successors {
            let to = plan.frame(plan.blocks[target.0].entry).clone();
            let mut actions = Vec::new();
            for i in 0..8 {
                if from.gpr[i] != to.gpr[i] {
                    actions.push(EdgeAction::Copy(ParallelCopy {
                        source: from.gpr[i],
                        destination: to.gpr[i],
                    }));
                }
            }
            for i in 0..8 {
                let source_values = from.fp.payload[i].values();
                let destination_values = to.fp.payload[i].values();
                if source_values.len() == destination_values.len() {
                    for (source, destination) in source_values.into_iter().zip(destination_values) {
                        if source != destination {
                            actions.push(EdgeAction::Copy(ParallelCopy {
                                source,
                                destination,
                            }));
                        }
                    }
                }
            }
            let fp_rebase = plan.shape(source) != plan.shape(plan.blocks[target.0].entry);
            plan.blocks[index].edges.push(EdgePlan {
                target,
                frame: source,
                actions,
                fp_rebase,
            });
        }
    }
}

fn operation_address(operation: Operation) -> Option<EffectiveAddress> {
    match operation {
        Operation::X87 { address, .. } => address,
        Operation::Lea { address, .. }
        | Operation::LeaWord { address, .. }
        | Operation::LoadByte { address, .. }
        | Operation::LoadWord { address, .. }
        | Operation::StoreByte { address, .. }
        | Operation::StoreWord { address, .. }
        | Operation::MemoryUnaryByte { address, .. }
        | Operation::MemoryUnaryWord { address, .. } => Some(address),
        Operation::Move {
            destination: Location32::Memory(address),
            ..
        }
        | Operation::Binary {
            destination: Location32::Memory(address),
            ..
        }
        | Operation::Unary {
            destination: Location32::Memory(address),
            ..
        } => Some(address),
        Operation::Move {
            source: Value32::Memory(address),
            ..
        }
        | Operation::Binary {
            source: Value32::Memory(address),
            ..
        } => Some(address),
        Operation::Jump {
            target: BranchTarget::Indirect(Location32::Memory(address)),
        }
        | Operation::Call {
            target: BranchTarget::Indirect(Location32::Memory(address)),
        } => Some(address),
        _ => None,
    }
}
