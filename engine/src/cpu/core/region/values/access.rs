use super::{BlockId, FrameState, Region, RegionPlan, ValueId, fp};
use crate::cpu::x86::{
    X87Format, X87Operation,
    ir::{
        AddressSize, BinaryKind, EffectiveAddress, Location32, Operation, SmallSource, SmallWidth,
        Value32,
    },
    simd::SimdOperand,
};
use std::collections::BTreeMap;

const MAX_GROUPS: usize = 8;

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
    pub authority: u32,
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

// Only explicitly addressed, statically sized accesses participate. In particular,
// strings, implicit stacks, and bit-index-adjusted addresses are not approximated.
fn operands(operation: Operation) -> Vec<Access> {
    let read = |address, width| Access::new(address, width, true, false);
    let write = |address, width| Access::new(address, width, false, true);
    let modify = |address, width| Access::new(address, width, true, true);
    match operation {
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

type Key = (u32, Option<ValueId>, Option<ValueId>, u8, bool, usize);

fn key(authority: u32, frame: &FrameState, address: EffectiveAddress) -> Key {
    (
        authority,
        address.base.map(|r| frame.gpr[r.index()]),
        address.index.map(|r| frame.gpr[r.index()]),
        address.scale,
        address.address_size == AddressSize::Bits16,
        address.resolved_segment().index(),
    )
}

pub(super) fn analyze(region: &Region, plan: &RegionPlan) -> Vec<BlockAccessPlan> {
    region
        .blocks
        .iter()
        .enumerate()
        .map(|(block_index, block)| {
            let id = BlockId(block_index);
            let mut candidates = BTreeMap::<Key, AccessGroup>::new();
            let mut authority = 0;
            for instruction in &block.instructions {
                let Some(operation) = instruction.operation else {
                    authority += 1;
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
                    authority += 1;
                    continue;
                }
                let frame = plan.frame(plan.instruction(id, instruction.prefix).before);
                for access in operands(operation) {
                    let group_key = key(authority, frame, access.address);
                    let group = candidates.entry(group_key).or_insert_with(|| AccessGroup {
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
                    });
                    let delta = i64::from(
                        access
                            .address
                            .displacement
                            .wrapping_sub(group.anchor.displacement) as i32,
                    );
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
            }
            let mut groups: Vec<_> = candidates
                .into_values()
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
