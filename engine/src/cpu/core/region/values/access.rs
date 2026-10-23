use super::{BlockId, FrameState, Region, RegionPlan, ValueId, fp};
use crate::cpu::x86::{
    ControlOperation, Register32, SegmentRegister, X87Format, X87Operation,
    ir::{
        AddressSize, BinaryKind, BranchTarget, EffectiveAddress, Location32, Operation,
        SmallSource, SmallWidth, UnaryKind, Value32,
    },
    simd::SimdOperand,
};
use std::collections::BTreeMap;

const MAX_GROUPS: usize = 8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::cpu::core) struct AccessEpoch(u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cpu::core) struct AccessMember {
    pub group: u8,
    pub prefix: u32,
    pub address: EffectiveAddress,
    pub width: u32,
    pub read: bool,
    pub write: bool,
    pub alignment: u32,
    // Signed modular displacement relative to the group's actual anchor EA.
    pub delta: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::cpu::core) struct AccessGroup {
    pub slot: u8,
    pub anchor: EffectiveAddress,
    pub base: Option<ValueId>,
    pub index: Option<ValueId>,
    // This authority is block-local and independent of data-write effects.
    // Runtime canonical calls must additionally invalidate all descriptor slots.
    pub authority: AccessEpoch,
    pub min_delta: i64,
    pub max_end: i64,
    pub permissions: u8,
    pub alignment_mask: u32,
    pub alignment_residue: u32,
    pub alignment_compatible: bool,
    pub first_prefix: u32,
    pub last_prefix: u32,
    pub members: Vec<AccessMember>,
}

impl AccessGroup {
    pub fn alignment_admission(&self) -> (u32, u32, bool) {
        (
            self.alignment_mask,
            self.alignment_residue,
            self.alignment_compatible,
        )
    }
}

#[derive(Default)]
pub(super) struct BlockAccessPlan {
    pub groups: Vec<AccessGroup>,
    members: Vec<AccessMember>,
}

impl BlockAccessPlan {
    pub fn member(
        &self,
        prefix: u32,
        address: EffectiveAddress,
        width: u32,
        write: bool,
    ) -> Option<&AccessMember> {
        let first = self
            .members
            .partition_point(|member| member.prefix < prefix);
        self.members[first..]
            .iter()
            .take_while(|member| member.prefix == prefix)
            .find(|member| {
                member.address == address
                    && member.width == width
                    && if write { member.write } else { member.read }
            })
    }
}

#[derive(Clone, Copy)]
struct Access {
    address: EffectiveAddress,
    width: u32,
    read: bool,
    write: bool,
    alignment: u32,
}

impl Access {
    fn new(address: EffectiveAddress, width: u32, read: bool, write: bool) -> Self {
        Self {
            address,
            width,
            read,
            write,
            alignment: 1,
        }
    }
}

fn stack(base: Register32, displacement: u32) -> EffectiveAddress {
    EffectiveAddress {
        base: Some(base),
        displacement,
        segment: Some(SegmentRegister::Ss),
        ..Default::default()
    }
}

fn popped_destination(mut address: EffectiveAddress, width: u32) -> EffectiveAddress {
    if address.base == Some(Register32::Esp) {
        address.displacement = address.displacement.wrapping_add(width);
    }
    address
}

// statically sized actual accesses only; stack offsets refer to the instruction's
// incoming register values, including pop's post-increment destination address.
// strings and dynamic bit-index adjustments keep their ordinary precise paths.
fn operands(operation: Operation) -> Vec<Access> {
    let read = |address, width| Access::new(address, width, true, false);
    let write = |address, width| Access::new(address, width, false, true);
    let modify = |address, width| Access::new(address, width, true, true);
    let push = |width| write(stack(Register32::Esp, 0u32.wrapping_sub(width)), width);
    let pop = |width| read(stack(Register32::Esp, 0), width);
    match operation {
        Operation::Push { source } => {
            let mut accesses = Vec::new();
            if let Value32::Memory(address) = source {
                accesses.push(read(address, 4));
            }
            accesses.push(push(4));
            accesses
        }
        Operation::PushWord { .. } | Operation::PushFlagsWord => vec![push(2)],
        Operation::PushMemoryWord { source } => vec![read(source, 2), push(2)],
        Operation::PushFlags | Operation::EnterZero => vec![push(4)],
        Operation::EnterZeroWord => vec![push(2)],
        Operation::Pop { destination } => {
            let mut accesses = vec![pop(4)];
            if let Location32::Memory(address) = destination {
                accesses.push(write(popped_destination(address, 4), 4));
            }
            accesses
        }
        Operation::PopWord { .. } | Operation::ReturnWord { .. } => vec![pop(2)],
        Operation::PopMemoryWord { destination } => {
            vec![pop(2), write(popped_destination(destination, 2), 2)]
        }
        Operation::Return { .. } => vec![pop(4)],
        Operation::Control(ControlOperation::PopFlags { word }) => {
            vec![pop(if word { 2 } else { 4 })]
        }
        Operation::Call { target } => {
            let mut accesses = Vec::new();
            if let BranchTarget::Indirect(Location32::Memory(address)) = target {
                accesses.push(read(address, 4));
            }
            accesses.push(push(4));
            accesses
        }
        Operation::CallWord { .. } | Operation::CallRegisterWord { .. } => vec![push(2)],
        Operation::ReadCallWord { address } => vec![read(address, 2), push(2)],
        Operation::Leave => vec![read(stack(Register32::Ebp, 0), 4)],
        Operation::LeaveWord => vec![read(stack(Register32::Ebp, 0), 2)],
        Operation::Move {
            destination,
            source,
        } => {
            let mut operands = Vec::new();
            if let Value32::Memory(address) = source {
                operands.push(read(address, 4));
            }
            if let Location32::Memory(address) = destination {
                operands.push(write(address, 4));
            }
            operands
        }
        Operation::Binary {
            kind,
            destination,
            source,
        } => {
            let mut operands = Vec::new();
            if let Value32::Memory(address) = source {
                operands.push(read(address, 4));
            }
            if let Location32::Memory(address) = destination {
                operands.push(if matches!(kind, BinaryKind::Cmp | BinaryKind::Test) {
                    read(address, 4)
                } else {
                    modify(address, 4)
                });
            }
            operands
        }
        Operation::Unary {
            destination: Location32::Memory(address),
            ..
        }
        | Operation::MemoryExchange { address, .. }
        | Operation::MemoryExchangeAdd { address, .. }
        | Operation::MemoryCompareExchange { address, .. }
        | Operation::LockedMemoryOr32 { address, .. }
        | Operation::LockedMemoryAnd32 { address, .. }
        | Operation::LockedMemoryXor32 { address, .. }
        | Operation::LockedMemoryOrRegister32 { address, .. }
        | Operation::LockedMemoryAndRegister32 { address, .. }
        | Operation::LockedMemoryXorRegister32 { address, .. } => vec![modify(address, 4)],
        Operation::LoadByte { address, .. }
        | Operation::ReadLogicalByte { address, .. }
        | Operation::ReadArithmeticByte { address, .. }
        | Operation::ReadCompareByte { address, .. }
        | Operation::MemoryPredicateByte { address, .. }
        | Operation::ReadExtendByteToWord { address, .. } => vec![read(address, 1)],
        Operation::LoadWord { address, .. }
        | Operation::ReadLogicalWord { address, .. }
        | Operation::ReadArithmeticWord { address, .. }
        | Operation::ReadCompareWord { address, .. }
        | Operation::MemoryCompareWord { address, .. }
        | Operation::MemoryTestWord { address, .. }
        | Operation::ReadConditionalMoveWord { address, .. }
        | Operation::ReadSignedMultiplyWord { address, .. }
        | Operation::ReadSignedMultiplyImmediateWord { address, .. } => vec![read(address, 2)],
        Operation::ReadConditionalMove { address, .. } => vec![read(address, 4)],
        Operation::StoreByte { address, .. } | Operation::MemorySetByte { address, .. } => {
            vec![write(address, 1)]
        }
        Operation::StoreWord { address, .. } => vec![write(address, 2)],
        Operation::MemoryLogicalByte { address, .. }
        | Operation::MemoryArithmeticByte { address, .. }
        | Operation::MemoryUnaryByte { address, .. }
        | Operation::MemoryExchangeByte { address, .. }
        | Operation::MemoryExchangeAddByte { address, .. }
        | Operation::MemoryCompareExchangeByte { address, .. } => vec![modify(address, 1)],
        Operation::MemoryLogicalWord { address, .. }
        | Operation::MemoryArithmeticWord { address, .. }
        | Operation::MemoryUnaryWord { address, .. }
        | Operation::MemoryExchangeWord { address, .. }
        | Operation::MemoryExchangeAddWord { address, .. }
        | Operation::MemoryCompareExchangeWord { address, .. } => vec![modify(address, 2)],
        Operation::AtomicMemory { address, width, .. } if matches!(width, 1 | 2 | 4) => {
            vec![modify(address, u32::from(width))]
        }
        Operation::Extend {
            source: SmallSource::Memory { address, width },
            ..
        } => {
            vec![read(address, if width == SmallWidth::Byte { 1 } else { 2 })]
        }
        Operation::X87 {
            operation,
            address: Some(address),
            ..
        } => match operation {
            X87Operation::Load(X87Format::Float32)
            | X87Operation::BinaryMemory {
                format: X87Format::Float32,
                ..
            } => vec![read(address, 4)],
            X87Operation::Store {
                format: X87Format::Float32,
                truncate: false,
                ..
            } => vec![write(address, 4)],
            X87Operation::Store {
                format: X87Format::Float64,
                truncate: false,
                ..
            } => vec![write(address, 8)],
            _ => Vec::new(),
        },
        Operation::Simd(op) if crate::cpu::core::emitter::preserves_fp_success(&operation) => {
            let mut operands = Vec::new();
            if let SimdOperand::Memory(address) = op.source {
                operands.push(read(address, 16));
            }
            if let SimdOperand::Memory(address) = op.destination {
                operands.push(write(address, 16));
            }
            for operand in &mut operands {
                operand.alignment = if op.aligned { 16 } else { 1 };
            }
            operands
        }
        _ => Vec::new(),
    }
}

#[derive(Clone, Copy)]
struct Affine {
    root: ValueId,
    displacement: u32,
}

impl Affine {
    fn incoming(root: ValueId) -> Self {
        Self {
            root,
            displacement: 0,
        }
    }

    fn offset(self, displacement: u32) -> Self {
        Self {
            displacement: self.displacement.wrapping_add(displacement),
            ..self
        }
    }
}

type Key = (
    AccessEpoch,
    Option<ValueId>,
    Option<ValueId>,
    u8,
    bool,
    usize,
);

fn normalized(
    epoch: AccessEpoch,
    registers: &[Affine; 8],
    address: EffectiveAddress,
) -> (Key, u32) {
    let base = address.base.map(|r| registers[r.index()]);
    let index = address.index.map(|r| registers[r.index()]);
    let displacement = address
        .displacement
        .wrapping_add(base.map_or(0, |value| value.displacement))
        .wrapping_add(index.map_or(0, |value| {
            value.displacement.wrapping_mul(u32::from(address.scale))
        }));
    (
        (
            epoch,
            base.map(|value| value.root),
            index.map(|value| value.root),
            address.scale,
            address.address_size == AddressSize::Bits16,
            address.resolved_segment().index(),
        ),
        displacement,
    )
}

// data writes advance MemoryEffect, but leave mapping/permission/alias authority
// intact. only exact affine register changes preserve an address family here.
fn advance(
    registers: &mut [Affine; 8],
    before: &FrameState,
    after: &FrameState,
    operation: Operation,
) {
    let old = *registers;
    let mut next = std::array::from_fn(|i| {
        if before.gpr[i] == after.gpr[i] {
            old[i]
        } else {
            Affine::incoming(after.gpr[i])
        }
    });
    let stack_delta = match operation {
        Operation::Push { .. } | Operation::PushFlags | Operation::Call { .. } => Some(-4i32),
        Operation::PushWord { .. }
        | Operation::PushMemoryWord { .. }
        | Operation::PushFlagsWord
        | Operation::CallWord { .. }
        | Operation::CallRegisterWord { .. }
        | Operation::ReadCallWord { .. } => Some(-2),
        Operation::Pop { .. } => Some(4),
        Operation::PopWord { .. } | Operation::PopMemoryWord { .. } => Some(2),
        Operation::Return { stack_adjust } => Some(4 + i32::from(stack_adjust)),
        Operation::ReturnWord { stack_adjust } => Some(2 + i32::from(stack_adjust)),
        Operation::Control(ControlOperation::PopFlags { word }) => Some(if word { 2 } else { 4 }),
        _ => None,
    };
    if let Some(delta) = stack_delta {
        // these operations preserve every other register, except the actual pop
        // destination. a pop into ESP establishes an unknown new stack root.
        next = old;
        next[4] = old[4].offset(delta as u32);
        match operation {
            Operation::Pop {
                destination: Location32::Register(r),
            }
            | Operation::PopWord { destination: r } => {
                next[r.index()] = Affine::incoming(after.gpr[r.index()])
            }
            _ => {}
        }
    }
    match operation {
        Operation::Move {
            destination: Location32::Register(r),
            source: Value32::Register(source),
        } => {
            next[r.index()] = old[source.index()];
        }
        Operation::Binary {
            kind: BinaryKind::Add | BinaryKind::Sub,
            destination: Location32::Register(r),
            source: Value32::Immediate(value),
        } => {
            let delta = if matches!(
                operation,
                Operation::Binary {
                    kind: BinaryKind::Sub,
                    ..
                }
            ) {
                value.wrapping_neg()
            } else {
                value
            };
            next[r.index()] = old[r.index()].offset(delta);
        }
        Operation::Unary {
            kind: UnaryKind::Inc | UnaryKind::Dec,
            destination: Location32::Register(r),
        } => {
            next[r.index()] = old[r.index()].offset(
                if matches!(
                    operation,
                    Operation::Unary {
                        kind: UnaryKind::Dec,
                        ..
                    }
                ) {
                    u32::MAX
                } else {
                    1
                },
            );
        }
        Operation::Lea {
            destination,
            address,
        } if address.address_size == AddressSize::Bits32 && address.index.is_none() => {
            if let Some(base) = address.base {
                next[destination.index()] = old[base.index()].offset(address.displacement);
            }
        }
        Operation::EnterZero | Operation::EnterZeroWord => {
            next = old;
            next[4] = old[4].offset(if operation == Operation::EnterZero {
                -4i32
            } else {
                -2i32
            } as u32);
            next[5] = if operation == Operation::EnterZero {
                next[4]
            } else {
                Affine::incoming(after.gpr[5])
            };
        }
        Operation::Leave | Operation::LeaveWord => {
            next = old;
            next[4] = old[5].offset(if operation == Operation::Leave { 4 } else { 2 });
            next[5] = Affine::incoming(after.gpr[5]);
        }
        _ => {}
    }
    *registers = next;
}

pub(super) fn analyze(region: &Region, plan: &RegionPlan) -> Vec<BlockAccessPlan> {
    region
        .blocks
        .iter()
        .enumerate()
        .map(|(block_index, block)| {
            let id = BlockId(block_index);
            let mut candidates = BTreeMap::<Key, (u32, AccessGroup)>::new();
            let mut authority = AccessEpoch::default();
            let mut registers = plan.frame(plan.block(id).entry).gpr.map(Affine::incoming);
            for instruction in &block.instructions {
                let Some(operation) = instruction.operation else {
                    authority.0 += 1;
                    let after = plan.frame(plan.instruction(id, instruction.prefix).after);
                    registers = after.gpr.map(Affine::incoming);
                    continue;
                };
                if instruction.gate.is_some()
                    || fp::canonical_numeric(operation)
                    || matches!(
                        operation,
                        Operation::String { .. } | Operation::RepeatString { .. }
                    )
                    || !crate::cpu::core::emitter::supports(&operation)
                {
                    authority.0 += 1;
                    let after = plan.frame(plan.instruction(id, instruction.prefix).after);
                    registers = after.gpr.map(Affine::incoming);
                    continue;
                }
                let instruction_plan = plan.instruction(id, instruction.prefix);
                let frame = plan.frame(instruction_plan.before);
                for access in operands(operation) {
                    let (group_key, normalized_delta) =
                        normalized(authority, &registers, access.address);
                    let (anchor_delta, group) = candidates.entry(group_key).or_insert_with(|| {
                        (
                            normalized_delta,
                            AccessGroup {
                                slot: 0,
                                anchor: access.address,
                                base: group_key.1,
                                index: group_key.2,
                                authority,
                                min_delta: 0,
                                max_end: 0,
                                permissions: 0,
                                alignment_mask: 0,
                                alignment_residue: 0,
                                alignment_compatible: true,
                                first_prefix: instruction.prefix,
                                last_prefix: instruction.prefix,
                                members: Vec::new(),
                            },
                        )
                    });
                    let delta = i64::from(normalized_delta.wrapping_sub(*anchor_delta) as i32);
                    group.min_delta = group.min_delta.min(delta);
                    group.max_end = group.max_end.max(delta + i64::from(access.width));
                    group.permissions |= u8::from(access.read) | (u8::from(access.write) << 1);
                    group.last_prefix = instruction.prefix;
                    group.members.push(AccessMember {
                        group: 0,
                        prefix: instruction.prefix,
                        address: access.address,
                        width: access.width,
                        read: access.read,
                        write: access.write,
                        alignment: access.alignment,
                        delta,
                    });
                }
                advance(
                    &mut registers,
                    frame,
                    plan.frame(instruction_plan.after),
                    operation,
                );
            }
            let mut groups: Vec<_> = candidates
                .into_values()
                .map(|(_, group)| group)
                .filter(|group| {
                    group.first_prefix != group.last_prefix
                        && group.max_end - group.min_delta <= 4096
                })
                .collect();
            groups.sort_unstable_by_key(|group| group.first_prefix);
            groups.truncate(MAX_GROUPS);
            let mut members = Vec::new();
            for (slot, group) in groups.iter_mut().enumerate() {
                group.slot = slot as u8;
                let strongest = group
                    .members
                    .iter()
                    .max_by_key(|member| member.alignment)
                    .unwrap();
                group.alignment_mask = strongest.alignment - 1;
                group.alignment_residue = 0u32
                    .wrapping_sub((strongest.delta - group.min_delta) as u32)
                    & group.alignment_mask;
                group.alignment_compatible = group.members.iter().all(|member| {
                    let mask = member.alignment - 1;
                    group.alignment_residue & mask
                        == 0u32.wrapping_sub((member.delta - group.min_delta) as u32) & mask
                });
                for member in &mut group.members {
                    member.group = slot as u8;
                }
                members.extend_from_slice(&group.members);
            }
            members.sort_unstable_by_key(|member| member.prefix);
            BlockAccessPlan { groups, members }
        })
        .collect()
}

#[cfg(test)]
mod tests;
