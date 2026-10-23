use super::*;
use crate::cpu::x86::{
    X87Format,
    ir::{BinaryKind, CountBranchKind, Location32, Operation, UnaryKind, Value32},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Facts {
    pub gpr: [Option<u32>; 8],
    pub native: [bool; 8],
    pub tags: [Option<bool>; 8], // true occupied, false empty; class bits remain exact inputs.
    pub control_admitted: bool,
}

// Unvisited predecessors are distinct from unknown facts. This lets an unchanged
// loop carry a preheader constant; conflicting backedge values meet to unknown.
// External entries are checked by adapters and are not fictitious CFG predecessors.
pub(super) fn entries(region: &Region) -> Vec<Facts> {
    let mut inputs = vec![Facts::default(); region.blocks.len()];
    let mut outputs: Vec<Option<Facts>> = vec![None; region.blocks.len()];
    loop {
        let mut changed = false;
        for (index, block) in region.blocks.iter().enumerate() {
            let mut input = Facts::default();
            let mut predecessors = block.predecessors.iter().filter_map(|id| outputs[id.0]);
            if let Some(first) = predecessors.next() {
                input = first;
                for other in predecessors {
                    for i in 0..8 {
                        if input.gpr[i] != other.gpr[i] {
                            input.gpr[i] = None;
                        }
                        input.native[i] &= other.native[i];
                        if input.tags[i] != other.tags[i] {
                            input.tags[i] = None;
                        }
                    }
                    input.control_admitted &= other.control_admitted;
                }
            }
            let mut output = input;
            for instruction in &block.instructions {
                transfer(&mut output, instruction.operation);
            }
            changed |= inputs[index] != input || outputs[index] != Some(output);
            inputs[index] = input;
            outputs[index] = Some(output);
        }
        if !changed {
            return inputs;
        }
    }
}

pub(super) fn direct_fp(operation: X87Operation) -> bool {
    matches!(
        operation,
        X87Operation::Load(X87Format::Float32)
            | X87Operation::Store {
                format: X87Format::Float32 | X87Format::Float64,
                truncate: false,
                ..
            }
            | X87Operation::LoadRegister { .. }
            | X87Operation::StoreRegister { .. }
            | X87Operation::Exchange { .. }
            | X87Operation::Binary { .. }
            | X87Operation::BinaryMemory {
                format: X87Format::Float32,
                ..
            }
            | X87Operation::ChangeSign
            | X87Operation::Absolute
    )
}

pub(super) fn scalar_transfer(gpr: &mut [Option<u32>; 8], operation: Option<Operation>) {
    let source = |value: Value32, gpr: &[Option<u32>; 8]| match value {
        Value32::Immediate(value) => Some(value),
        Value32::Register(r) => gpr[r.index()],
        _ => None,
    };
    match operation {
        Some(Operation::Move {
            destination: Location32::Register(destination),
            source: value,
        }) => gpr[destination.index()] = source(value, gpr),
        Some(Operation::Move {
            destination: Location32::Memory(_),
            ..
        }) => {}
        Some(Operation::Lea {
            destination,
            address,
        }) => {
            gpr[destination.index()] = address_constant(address, |r| gpr[r.index()]);
        }
        Some(Operation::Binary {
            kind,
            destination,
            source: right,
        }) => {
            if matches!(kind, BinaryKind::Cmp | BinaryKind::Test) {
                return;
            }
            if let Location32::Register(register) = destination {
                let left = gpr[register.index()];
                gpr[register.index()] =
                    if kind == BinaryKind::Xor && right == Value32::Register(register) {
                        Some(0)
                    } else {
                        binary_constant(kind, left, source(right, gpr))
                    };
            }
        }
        Some(Operation::Unary {
            kind,
            destination: Location32::Register(register),
        }) => {
            gpr[register.index()] = gpr[register.index()].map(|value| match kind {
                UnaryKind::Inc => value.wrapping_add(1),
                UnaryKind::Dec => value.wrapping_sub(1),
                UnaryKind::Not => !value,
                UnaryKind::Neg => value.wrapping_neg(),
            });
        }
        Some(Operation::CountBranch { kind, .. } | Operation::CountBranchWord { kind, .. }) => {
            if kind != CountBranchKind::EcxZero {
                gpr[1] = gpr[1].map(|value| {
                    if matches!(operation, Some(Operation::CountBranchWord { .. })) {
                        value & !0xffff | u32::from((value as u16).wrapping_sub(1))
                    } else {
                        value.wrapping_sub(1)
                    }
                });
            }
        }
        Some(Operation::Push { .. } | Operation::PushFlags) => {
            gpr[4] = gpr[4].map(|v| v.wrapping_sub(4))
        }
        Some(Operation::Pop { destination }) => {
            gpr[4] = gpr[4].map(|v| v.wrapping_add(4));
            if let Location32::Register(register) = destination {
                gpr[register.index()] = None;
            }
        }
        Some(
            Operation::X87StatusToAx
            | Operation::X87 {
                operation: X87Operation::StatusToAx,
                ..
            },
        ) => gpr[0] = None,
        Some(
            Operation::X87 { .. }
            | Operation::Nop
            | Operation::StoreByte { .. }
            | Operation::StoreWord { .. }
            | Operation::Jump { .. }
            | Operation::ConditionalJump { .. }
            | Operation::Carry { .. }
            | Operation::Direction { .. }
            | Operation::MemoryUnaryByte { .. }
            | Operation::MemoryUnaryWord { .. },
        ) => {}
        _ => *gpr = [None; 8],
    }
}

pub(super) fn binary_constant(
    kind: BinaryKind,
    left: Option<u32>,
    right: Option<u32>,
) -> Option<u32> {
    Some(match (kind, left?, right?) {
        (BinaryKind::Add, l, r) => l.wrapping_add(r),
        (BinaryKind::Sub, l, r) => l.wrapping_sub(r),
        (BinaryKind::And, l, r) => l & r,
        (BinaryKind::Or, l, r) => l | r,
        (BinaryKind::Xor, l, r) => l ^ r,
        _ => return None,
    })
}

pub(super) fn address_constant(
    address: EffectiveAddress,
    mut register: impl FnMut(crate::cpu::x86::Register32) -> Option<u32>,
) -> Option<u32> {
    let mut value = address.displacement;
    if let Some(base) = address.base {
        value = value.wrapping_add(register(base)?);
    }
    if let Some(index) = address.index {
        value = value.wrapping_add(register(index)?.wrapping_mul(u32::from(address.scale)));
    }
    if address.address_size == crate::cpu::x86::ir::AddressSize::Bits16 {
        value &= 0xffff;
    }
    Some(value)
}

fn transfer(facts: &mut Facts, operation: Option<Operation>) {
    scalar_transfer(&mut facts.gpr, operation);
    let Some((operation, ..)) = operation.and_then(super::fp::operation) else {
        if operation.is_none_or(super::fp::canonical_numeric) {
            facts.native = [false; 8];
            facts.tags = [None; 8];
            facts.control_admitted = false;
        }
        return;
    };
    match operation {
        X87Operation::Load(X87Format::Float32) => {
            facts.native.rotate_right(1);
            facts.native[0] = true;
        }
        X87Operation::LoadRegister { index } => {
            let value = facts.native[index as usize];
            facts.native.rotate_right(1);
            facts.native[0] = value;
        }
        X87Operation::StoreRegister { index, pop } => {
            facts.native[index as usize] = facts.native[0];
            if pop {
                facts.native.rotate_left(1);
            }
        }
        X87Operation::Exchange { index } => facts.native.swap(0, index as usize),
        X87Operation::Store { pop, .. } if direct_fp(operation) => {
            if pop {
                facts.native.rotate_left(1);
            }
        }
        X87Operation::Binary {
            destination,
            source,
            pop,
            ..
        } => {
            facts.native[destination as usize] = true;
            facts.native[source as usize] = true;
            if pop {
                facts.native.rotate_left(1);
            }
        }
        X87Operation::BinaryMemory {
            format: X87Format::Float32,
            ..
        } => facts.native[0] = true,
        X87Operation::ChangeSign | X87Operation::Absolute => {}
        X87Operation::Wait | X87Operation::StatusToAx => {}
        _ => facts.native = [false; 8],
    }
    match operation {
        X87Operation::Load(X87Format::Float32) => {
            facts.tags.rotate_right(1);
            facts.tags[0] = Some(true);
        }
        X87Operation::LoadRegister { index } => {
            facts.tags[index as usize] = Some(true);
            facts.tags.rotate_right(1);
            facts.tags[0] = Some(true);
        }
        X87Operation::StoreRegister { index, pop } => {
            facts.tags[0] = Some(true);
            facts.tags[index as usize] = Some(true);
            if pop {
                facts.tags[0] = Some(false);
                facts.tags.rotate_left(1);
            }
        }
        X87Operation::Exchange { index } => {
            facts.tags[0] = Some(true);
            facts.tags[index as usize] = Some(true);
        }
        X87Operation::Store { pop, .. } if direct_fp(operation) => {
            facts.tags[0] = Some(true);
            if pop {
                facts.tags[0] = Some(false);
                facts.tags.rotate_left(1);
            }
        }
        X87Operation::Binary {
            destination,
            source,
            pop,
            ..
        } => {
            facts.tags[destination as usize] = Some(true);
            facts.tags[source as usize] = Some(true);
            facts.control_admitted = true;
            if pop {
                facts.tags[0] = Some(false);
                facts.tags.rotate_left(1);
            }
        }
        X87Operation::BinaryMemory {
            format: X87Format::Float32,
            ..
        } => {
            facts.tags[0] = Some(true);
            facts.control_admitted = true;
        }
        X87Operation::ChangeSign | X87Operation::Absolute => facts.tags[0] = Some(true),
        X87Operation::Wait | X87Operation::StatusToAx => {}
        _ => {
            facts.tags = [None; 8];
            facts.control_admitted = false;
        }
    }
}
