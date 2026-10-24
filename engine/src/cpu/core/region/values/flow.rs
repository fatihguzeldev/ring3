use super::*;
use crate::cpu::x86::{
    X87Format,
    ir::{BinaryKind, CountBranchKind, Location32, Operation, UnaryKind, Value32},
};
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Facts {
    pub gpr: [Option<u32>; 8],
    pub native: [bool; 8],
    pub bounded: [bool; 8],
    pub native24: [bool; 8],
    pub native24_in_single: [bool; 8],
    pub tags: [Option<bool>; 8], // true occupied, false empty; class bits remain exact inputs.
    pub control_admitted: bool,
}

// Bottom is per component: an unvisited second payload is not a known absence
// of a native proof. A component can only move Unvisited -> Known -> Unknown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fact<T> {
    Unvisited,
    Known(T),
    Unknown,
}

impl<T: Copy + Eq> Fact<T> {
    fn meet(&mut self, other: Self) -> bool {
        let next = match (*self, other) {
            (value, Self::Unvisited) | (Self::Unvisited, value) => value,
            (Self::Known(left), Self::Known(right)) if left == right => Self::Known(left),
            _ => Self::Unknown,
        };
        let changed = *self != next;
        *self = next;
        changed
    }

    fn finalize(&mut self) -> bool {
        if matches!(self, Self::Unvisited) {
            *self = Self::Unknown;
            true
        } else {
            false
        }
    }
}

impl<T> Fact<T> {
    fn from_option(value: Option<T>) -> Self {
        value.map_or(Self::Unknown, Self::Known)
    }

    fn into_option(self) -> Option<T> {
        match self {
            Self::Known(value) => Some(value),
            _ => None,
        }
    }

    fn map<U>(self, map: impl FnOnce(T) -> U) -> Fact<U> {
        match self {
            Self::Unvisited => Fact::Unvisited,
            Self::Known(value) => Fact::Known(map(value)),
            Self::Unknown => Fact::Unknown,
        }
    }

    fn zip<U>(self, other: Fact<U>) -> Fact<(T, U)> {
        match (self, other) {
            (Self::Unknown, _) | (_, Fact::Unknown) => Fact::Unknown,
            (Self::Unvisited, _) | (_, Fact::Unvisited) => Fact::Unvisited,
            (Self::Known(left), Fact::Known(right)) => Fact::Known((left, right)),
        }
    }
}

#[derive(Clone, Copy)]
struct PendingFacts {
    gpr: [Fact<u32>; 8],
    native: [Fact<()>; 8],
    bounded: [Fact<()>; 8],
    native24: [Fact<()>; 8],
    native24_in_single: [Fact<()>; 8],
    tags: [Fact<bool>; 8],
    control_admitted: Fact<()>,
}

impl PendingFacts {
    fn unvisited() -> Self {
        Self {
            gpr: [Fact::Unvisited; 8],
            native: [Fact::Unvisited; 8],
            bounded: [Fact::Unvisited; 8],
            native24: [Fact::Unvisited; 8],
            native24_in_single: [Fact::Unvisited; 8],
            tags: [Fact::Unvisited; 8],
            control_admitted: Fact::Unvisited,
        }
    }

    fn meet(&mut self, other: Self) -> bool {
        let mut changed = false;
        for i in 0..8 {
            changed |= self.gpr[i].meet(other.gpr[i]);
            changed |= self.native[i].meet(other.native[i]);
            changed |= self.bounded[i].meet(other.bounded[i]);
            changed |= self.native24[i].meet(other.native24[i]);
            changed |= self.native24_in_single[i].meet(other.native24_in_single[i]);
            changed |= self.tags[i].meet(other.tags[i]);
        }
        changed | self.control_admitted.meet(other.control_admitted)
    }

    fn finalize(&mut self) -> bool {
        let mut changed = false;
        for i in 0..8 {
            changed |= self.gpr[i].finalize();
            changed |= self.native[i].finalize();
            changed |= self.bounded[i].finalize();
            changed |= self.native24[i].finalize();
            changed |= self.native24_in_single[i].finalize();
            changed |= self.tags[i].finalize();
        }
        changed | self.control_admitted.finalize()
    }

    fn complete(self) -> Facts {
        Facts {
            gpr: self.gpr.map(Fact::into_option),
            native: self.native.map(|fact| matches!(fact, Fact::Known(()))),
            bounded: self.bounded.map(|fact| matches!(fact, Fact::Known(()))),
            native24: self.native24.map(|fact| matches!(fact, Fact::Known(()))),
            native24_in_single: self
                .native24_in_single
                .map(|fact| matches!(fact, Fact::Known(()))),
            tags: self.tags.map(Fact::into_option),
            control_admitted: matches!(self.control_admitted, Fact::Known(())),
        }
    }
}

pub(super) fn entries(region: &Region) -> Vec<Facts> {
    let mut inputs = vec![PendingFacts::unvisited(); region.blocks.len()];
    let mut queued = vec![true; region.blocks.len()];
    let mut pending: VecDeque<_> = (0..region.blocks.len()).map(BlockId).collect();
    for (index, block) in region.blocks.iter().enumerate() {
        if block.predecessors.is_empty() {
            inputs[index].finalize();
        }
    }
    loop {
        while let Some(id) = pending.pop_front() {
            queued[id.0] = false;
            let block = &region.blocks[id.0];
            let mut output = inputs[id.0];
            for instruction in &block.instructions {
                transfer(&mut output, instruction.operation);
            }
            for &successor in &block.successors {
                if inputs[successor.0].meet(output) && !queued[successor.0] {
                    pending.push_back(successor);
                    queued[successor.0] = true;
                }
            }
        }
        // A source-less cycle can leave some fields bottom. Its actual unknown
        // state must propagate into downstream joins before any facts escape.
        // This only advances the same finite lattice; there is no sweep cap.
        for (index, input) in inputs.iter_mut().enumerate() {
            if input.finalize() && !queued[index] {
                pending.push_back(BlockId(index));
                queued[index] = true;
            }
        }
        if pending.is_empty() {
            break;
        }
    }
    // Cyclic entry hints remain guarded by actual imports. An external entry is
    // not a fictitious unknown predecessor that erases generated loop proofs.
    inputs.into_iter().map(PendingFacts::complete).collect()
}

pub(super) fn direct_fp(operation: X87Operation) -> bool {
    matches!(
        operation,
        X87Operation::Load(X87Format::Float32 | X87Format::Float64)
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
    ) || matches!(
        operation,
        X87Operation::Compare {
            index: 0..=7,
            pop: 0..=2,
            integer_flags: false,
            ..
        } | X87Operation::CompareMemory {
            format: X87Format::Float32 | X87Format::Float64,
            ..
        }
    )
}

/// Operations with an executable typed consumer, including effects that do not
/// use the native arithmetic domain.
pub(super) fn typed_fp(operation: X87Operation) -> bool {
    direct_fp(operation)
        || matches!(
            operation,
            X87Operation::LoadControl
                | X87Operation::StoreControl
                | X87Operation::Load(
                    X87Format::Integer16 | X87Format::Integer32 | X87Format::Integer64
                )
                | X87Operation::Store {
                    format: X87Format::Integer16 | X87Format::Integer32 | X87Format::Integer64,
                    ..
                }
        )
}

pub(super) fn scalar_transfer(gpr: &mut [Option<u32>; 8], operation: Option<Operation>) {
    let mut pending = (*gpr).map(Fact::from_option);
    scalar_transfer_values(&mut pending, operation);
    *gpr = pending.map(Fact::into_option);
}

fn scalar_transfer_values(gpr: &mut [Fact<u32>; 8], operation: Option<Operation>) {
    let source = |value: Value32, gpr: &[Fact<u32>; 8]| match value {
        Value32::Immediate(value) => Fact::Known(value),
        Value32::Register(r) => gpr[r.index()],
        _ => Fact::Unknown,
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
            gpr[destination.index()] = address_value(address, |r| gpr[r.index()]);
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
                        Fact::Known(0)
                    } else {
                        binary_value(kind, left, source(right, gpr))
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
        Some(Operation::Push { .. } | Operation::PushFlags | Operation::Call { .. }) => {
            gpr[4] = gpr[4].map(|v| v.wrapping_sub(4))
        }
        Some(
            Operation::PushWord { .. }
            | Operation::PushMemoryWord { .. }
            | Operation::PushFlagsWord
            | Operation::CallWord { .. }
            | Operation::CallRegisterWord { .. }
            | Operation::ReadCallWord { .. },
        ) => gpr[4] = gpr[4].map(|v| v.wrapping_sub(2)),
        Some(Operation::Return { stack_adjust }) => {
            gpr[4] = gpr[4].map(|v| v.wrapping_add(4 + u32::from(stack_adjust)))
        }
        Some(Operation::ReturnWord { stack_adjust }) => {
            gpr[4] = gpr[4].map(|v| v.wrapping_add(2 + u32::from(stack_adjust)))
        }
        Some(Operation::Pop { destination }) => {
            gpr[4] = gpr[4].map(|v| v.wrapping_add(4));
            if let Location32::Register(register) = destination {
                gpr[register.index()] = Fact::Unknown;
            }
        }
        Some(Operation::PopWord { destination }) => {
            gpr[4] = gpr[4].map(|v| v.wrapping_add(2));
            gpr[destination.index()] = Fact::Unknown;
        }
        Some(Operation::PopMemoryWord { .. }) => gpr[4] = gpr[4].map(|v| v.wrapping_add(2)),
        Some(Operation::Control(crate::cpu::x86::ControlOperation::PopFlags { word })) => {
            gpr[4] = gpr[4].map(|v| v.wrapping_add(if word { 2 } else { 4 }))
        }
        Some(Operation::EnterZero | Operation::EnterZeroWord) => {
            let word = matches!(operation, Some(Operation::EnterZeroWord));
            gpr[4] = gpr[4].map(|v| v.wrapping_sub(if word { 2 } else { 4 }));
            gpr[5] = if word {
                gpr[5]
                    .zip(gpr[4])
                    .map(|(old, stack)| old & !0xffff | stack & 0xffff)
            } else {
                gpr[4]
            };
        }
        Some(Operation::Leave | Operation::LeaveWord) => {
            gpr[4] = gpr[5].map(|v| {
                v.wrapping_add(if matches!(operation, Some(Operation::LeaveWord)) {
                    2
                } else {
                    4
                })
            });
            gpr[5] = Fact::Unknown;
        }
        Some(
            Operation::X87StatusToAx
            | Operation::X87 {
                operation: X87Operation::StatusToAx,
                ..
            },
        ) => gpr[0] = Fact::Unknown,
        Some(
            Operation::X87 { .. }
            | Operation::Nop
            | Operation::StoreByte { .. }
            | Operation::StoreWord { .. }
            | Operation::Jump { .. }
            | Operation::JumpWord { .. }
            | Operation::ReadJumpWord { .. }
            | Operation::ConditionalJump { .. }
            | Operation::Carry { .. }
            | Operation::Direction { .. }
            | Operation::MemoryUnaryByte { .. }
            | Operation::MemoryUnaryWord { .. },
        ) => {}
        _ => *gpr = [Fact::Unknown; 8],
    }
}

pub(super) fn binary_constant(
    kind: BinaryKind,
    left: Option<u32>,
    right: Option<u32>,
) -> Option<u32> {
    binary_value(kind, Fact::from_option(left), Fact::from_option(right)).into_option()
}

fn binary_value(kind: BinaryKind, left: Fact<u32>, right: Fact<u32>) -> Fact<u32> {
    if !matches!(
        kind,
        BinaryKind::Add | BinaryKind::Sub | BinaryKind::And | BinaryKind::Or | BinaryKind::Xor
    ) {
        return Fact::Unknown;
    }
    left.zip(right).map(|(l, r)| match kind {
        BinaryKind::Add => l.wrapping_add(r),
        BinaryKind::Sub => l.wrapping_sub(r),
        BinaryKind::And => l & r,
        BinaryKind::Or => l | r,
        BinaryKind::Xor => l ^ r,
        _ => unreachable!(),
    })
}

pub(super) fn address_constant(
    address: EffectiveAddress,
    mut register: impl FnMut(crate::cpu::x86::Register32) -> Option<u32>,
) -> Option<u32> {
    address_value(address, |r| Fact::from_option(register(r))).into_option()
}

fn address_value(
    address: EffectiveAddress,
    mut register: impl FnMut(crate::cpu::x86::Register32) -> Fact<u32>,
) -> Fact<u32> {
    let mut value = Fact::Known(address.displacement);
    if let Some(base) = address.base {
        value = value
            .zip(register(base))
            .map(|(value, base)| value.wrapping_add(base));
    }
    if let Some(index) = address.index {
        value = value
            .zip(register(index))
            .map(|(value, index)| value.wrapping_add(index.wrapping_mul(u32::from(address.scale))));
    }
    if address.address_size == crate::cpu::x86::ir::AddressSize::Bits16 {
        value = value.map(|value| value & 0xffff);
    }
    value
}

fn transfer(facts: &mut PendingFacts, operation: Option<Operation>) {
    scalar_transfer_values(&mut facts.gpr, operation);
    let Some((operation, ..)) = operation.and_then(super::fp::operation) else {
        if operation.is_none_or(super::fp::canonical_numeric) {
            facts.native = [Fact::Unknown; 8];
            facts.bounded = [Fact::Unknown; 8];
            facts.native24 = [Fact::Unknown; 8];
            facts.native24_in_single = [Fact::Unknown; 8];
            facts.tags = [Fact::Unknown; 8];
            facts.control_admitted = Fact::Unknown;
        }
        return;
    };
    if matches!(operation, X87Operation::StoreControl) {
        return;
    }
    if matches!(operation, X87Operation::LoadControl) {
        // Payload representation and independent tag facts survive a CW write;
        // stronger numerical/environment promises do not cross its epoch.
        facts.bounded = [Fact::Unknown; 8];
        facts.native24 = [Fact::Unknown; 8];
        facts.native24_in_single = [Fact::Unknown; 8];
        facts.control_admitted = Fact::Unknown;
        return;
    }
    if matches!(operation, X87Operation::Load(X87Format::Float64)) {
        for known in [
            &mut facts.native,
            &mut facts.bounded,
            &mut facts.native24,
            &mut facts.native24_in_single,
        ] {
            known.rotate_right(1);
        }
        facts.native[0] = Fact::Known(());
        facts.bounded[0] = Fact::Unknown;
        facts.native24[0] = Fact::Unknown;
        facts.native24_in_single[0] = Fact::Unknown;
        facts.tags.rotate_right(1);
        facts.tags[0] = Fact::Known(true);
        return;
    }
    let compare = match operation {
        X87Operation::Compare {
            index,
            pop,
            integer_flags: false,
            ..
        } if index < 8 && pop <= 2 => {
            facts.tags[index as usize] = Fact::Known(true);
            Some(pop)
        }
        X87Operation::CompareMemory {
            format: X87Format::Float32 | X87Format::Float64,
            pop,
        } => Some(u8::from(pop)),
        _ => None,
    };
    if let Some(pop) = compare {
        facts.tags[0] = Fact::Known(true);
        for _ in 0..pop {
            facts.tags[0] = Fact::Known(false);
            facts.tags.rotate_left(1);
            for known in [
                &mut facts.native,
                &mut facts.bounded,
                &mut facts.native24,
                &mut facts.native24_in_single,
            ] {
                known.rotate_left(1);
            }
        }
        return;
    }
    if let X87Operation::Load(
        format @ (X87Format::Integer16 | X87Format::Integer32 | X87Format::Integer64),
    ) = operation
    {
        for known in [
            &mut facts.native,
            &mut facts.bounded,
            &mut facts.native24,
            &mut facts.native24_in_single,
        ] {
            known.rotate_right(1);
        }
        facts.native[0] = if format == X87Format::Integer64 {
            Fact::Unknown
        } else {
            Fact::Known(())
        };
        facts.bounded[0] = facts.native[0];
        facts.native24[0] = if format == X87Format::Integer16 {
            Fact::Known(())
        } else {
            Fact::Unknown
        };
        facts.native24_in_single[0] = facts.native24[0];
        facts.tags.rotate_right(1);
        facts.tags[0] = Fact::Known(true);
        return;
    }
    if let X87Operation::Store {
        format: X87Format::Integer16 | X87Format::Integer32 | X87Format::Integer64,
        pop,
        ..
    } = operation
    {
        facts.tags[0] = Fact::Known(true);
        if pop {
            facts.tags[0] = Fact::Known(false);
            facts.tags.rotate_left(1);
            for known in [
                &mut facts.native,
                &mut facts.bounded,
                &mut facts.native24,
                &mut facts.native24_in_single,
            ] {
                known.rotate_left(1);
            }
        }
        return;
    }
    transfer_payload(&mut facts.native, operation, false);
    transfer_payload(&mut facts.bounded, operation, false);
    transfer_payload(&mut facts.native24, operation, true);
    transfer_payload(&mut facts.native24_in_single, operation, false);
    match operation {
        X87Operation::Load(X87Format::Float32) => {
            facts.tags.rotate_right(1);
            facts.tags[0] = Fact::Known(true);
        }
        X87Operation::LoadRegister { index } => {
            facts.tags[index as usize] = Fact::Known(true);
            facts.tags.rotate_right(1);
            facts.tags[0] = Fact::Known(true);
        }
        X87Operation::StoreRegister { index, pop } => {
            facts.tags[0] = Fact::Known(true);
            facts.tags[index as usize] = Fact::Known(true);
            if pop {
                facts.tags[0] = Fact::Known(false);
                facts.tags.rotate_left(1);
            }
        }
        X87Operation::Exchange { index } => {
            facts.tags[0] = Fact::Known(true);
            facts.tags[index as usize] = Fact::Known(true);
        }
        X87Operation::Store { pop, .. } if direct_fp(operation) => {
            facts.tags[0] = Fact::Known(true);
            if pop {
                facts.tags[0] = Fact::Known(false);
                facts.tags.rotate_left(1);
            }
        }
        X87Operation::Binary {
            destination,
            source,
            pop,
            ..
        } => {
            facts.tags[destination as usize] = Fact::Known(true);
            facts.tags[source as usize] = Fact::Known(true);
            facts.control_admitted = Fact::Known(());
            if pop {
                facts.tags[0] = Fact::Known(false);
                facts.tags.rotate_left(1);
            }
        }
        X87Operation::BinaryMemory {
            format: X87Format::Float32,
            ..
        } => {
            facts.tags[0] = Fact::Known(true);
            facts.control_admitted = Fact::Known(());
        }
        X87Operation::ChangeSign | X87Operation::Absolute => facts.tags[0] = Fact::Known(true),
        X87Operation::Wait | X87Operation::StatusToAx => {}
        _ => {
            facts.tags = [Fact::Unknown; 8];
            facts.control_admitted = Fact::Unknown;
        }
    }
}

fn transfer_payload(known: &mut [Fact<()>; 8], operation: X87Operation, native24: bool) {
    match operation {
        X87Operation::Load(X87Format::Float32) => {
            known.rotate_right(1);
            known[0] = Fact::Known(());
        }
        X87Operation::LoadRegister { index } => {
            let value = known[index as usize];
            known.rotate_right(1);
            known[0] = value;
        }
        X87Operation::StoreRegister { index, pop } => {
            known[index as usize] = known[0];
            if pop {
                known.rotate_left(1);
            }
        }
        X87Operation::Exchange { index } => known.swap(0, index as usize),
        X87Operation::Store { pop, .. } if direct_fp(operation) => {
            if pop {
                known.rotate_left(1);
            }
        }
        X87Operation::Binary {
            destination,
            source,
            pop,
            ..
        } => {
            if !native24 {
                known[source as usize] = Fact::Known(());
            }
            known[destination as usize] = if native24 {
                Fact::Unknown
            } else {
                Fact::Known(())
            };
            if pop {
                known.rotate_left(1);
            }
        }
        X87Operation::BinaryMemory {
            format: X87Format::Float32,
            ..
        } => {
            known[0] = if native24 {
                Fact::Unknown
            } else {
                Fact::Known(())
            }
        }
        X87Operation::ChangeSign | X87Operation::Absolute => {}
        X87Operation::Wait | X87Operation::StatusToAx => {}
        _ => *known = [Fact::Unknown; 8],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        cpu::{
            core::{CompileOptions, compile, program},
            x86::{
                Register32, X87Binary,
                ir::{BranchTarget, Condition},
            },
        },
        memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    };

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
            target: BranchTarget::Direct(GuestAddress(pc)),
        }
    }

    fn fp(operation: X87Operation) -> Operation {
        Operation::X87 {
            operation,
            address: matches!(operation, X87Operation::Load(_)).then_some(EffectiveAddress {
                base: Some(Register32::Ebx),
                ..EffectiveAddress::default()
            }),
            waiting: true,
            opcode: 0x103,
        }
    }

    fn constant(value: u32) -> Operation {
        Operation::Move {
            destination: Location32::Register(Register32::Eax),
            source: Value32::Immediate(value),
        }
    }

    #[test]
    fn real_fld_fxch_loop_with_late_preheader_converges() {
        let mut memory = AddressSpace::new(1).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        // Sorted block order differs from execution order: 1100 -> 1000 ->
        // 1080 -> 1080. The old solver seeds [true,false] at the swap loop
        // before visiting its real preheader, then swaps those facts forever.
        for (pc, bytes) in [
            (0x1000, &[0xd9, 0x03, 0xeb, 0x7c][..]),
            (0x1080, &[0xd9, 0xc9, 0xeb, 0xfc][..]),
            (0x1100, &[0xd9, 0x03, 0xe9, 0xf9, 0xfe, 0xff, 0xff][..]),
        ] {
            memory.write(GuestAddress(pc), bytes).unwrap();
        }
        let code = compile(&memory, GuestAddress(0x1100), CompileOptions::default()).unwrap();
        assert_eq!(
            code.instruction_pcs().collect::<Vec<_>>(),
            [0x1000, 0x1002, 0x1080, 0x1082, 0x1100, 0x1102]
        );
        assert_eq!(
            code.entrypoints(),
            [
                GuestAddress(0x1000),
                GuestAddress(0x1080),
                GuestAddress(0x1100)
            ]
        );
        assert!(code.is_current(&memory));
    }

    #[test]
    fn late_preheader_preserves_both_native_payloads_through_the_swap_backedge() {
        let load = fp(X87Operation::Load(X87Format::Float32));
        let blocks = [
            block(0x1000, &[load, jump(0x1080)]),
            block(
                0x1080,
                &[fp(X87Operation::Exchange { index: 1 }), jump(0x1080)],
            ),
            block(0x1100, &[load, jump(0x1000)]),
        ];
        let region = Region::from_blocks(&blocks, 0x1100).unwrap();
        let facts = entries(&region);
        let header = facts[1];
        assert_eq!(&header.native[..2], &[true, true]);
        assert_eq!(&header.bounded[..2], &[true, true]);
        assert_eq!(&header.native24[..2], &[true, true]);
        assert_eq!(&header.native24_in_single[..2], &[true, true]);
        assert_eq!(&header.tags[..2], &[Some(true), Some(true)]);
        assert_eq!(
            facts[2],
            Facts::default(),
            "the real external entry starts unknown"
        );
    }

    #[test]
    fn a_modifying_backedge_loses_a_preheader_constant_without_losing_unmodified_facts() {
        let load = fp(X87Operation::Load(X87Format::Float32));
        let blocks = [
            block(0x1000, &[constant(19), load, jump(0x1080)]),
            block(
                0x1080,
                &[
                    Operation::Unary {
                        kind: UnaryKind::Inc,
                        destination: Location32::Register(Register32::Eax),
                    },
                    jump(0x1080),
                ],
            ),
        ];
        let region = Region::from_blocks(&blocks, 0x1000).unwrap();
        let facts = entries(&region);
        assert_eq!(facts[1].gpr[0], None);
        assert!(facts[1].native[0]);
        assert!(facts[1].native24[0]);
        assert_eq!(facts[1].tags[0], Some(true));
    }

    #[test]
    fn root_backedges_preserve_generated_guarded_entry_hints() {
        let blocks = [block(
            0x1000,
            &[
                constant(19),
                fp(X87Operation::Binary {
                    kind: X87Binary::Add,
                    destination: 0,
                    source: 1,
                    pop: false,
                }),
                jump(0x1000),
            ],
        )];
        let region = Region::from_blocks(&blocks, 0x1000).unwrap();
        let facts = entries(&region)[0];
        assert_eq!(facts.gpr[0], Some(19));
        assert_eq!(&facts.native[..2], &[true, true]);
        assert_eq!(&facts.bounded[..2], &[true, true]);
        assert_eq!(&facts.native24[..2], &[false, false]);
        assert_eq!(&facts.native24_in_single[..2], &[true, true]);
        assert_eq!(&facts.tags[..2], &[Some(true), Some(true)]);
        assert!(facts.control_admitted);
    }

    #[test]
    fn disconnected_cycles_keep_only_their_own_generated_guarded_hints() {
        let blocks = [
            block(0x1000, &[Operation::Nop, jump(0x1000)]),
            block(0x1100, &[constant(7), jump(0x1100)]),
        ];
        let region = Region::from_blocks(&blocks, 0x1000).unwrap();
        let facts = entries(&region);
        assert_eq!(facts[0], Facts::default());
        assert_eq!(
            facts[1],
            Facts {
                gpr: [Some(7), None, None, None, None, None, None, None],
                ..Facts::default()
            }
        );
    }

    #[test]
    fn unresolved_cycle_unknowns_propagate_before_projecting_join_facts() {
        let blocks = [
            block(
                0x1000,
                &[
                    constant(19),
                    fp(X87Operation::Load(X87Format::Float32)),
                    jump(0x1101),
                ],
            ),
            // This separate public entry has no generating instruction. Its
            // unresolved EAX/native fields cannot be ignored at the join just
            // because the known predecessor reaches that join first.
            block(
                0x1100,
                &[Operation::ConditionalJump {
                    condition: Condition::Equal,
                    target: GuestAddress(0x1100),
                }],
            ),
            block(0x1101, &[Operation::Nop]),
        ];
        let region = Region::from_blocks(&blocks, 0x1000).unwrap();
        let facts = entries(&region);
        assert_eq!(facts[0], Facts::default());
        assert_eq!(facts[1], Facts::default());
        assert_eq!(facts[2], Facts::default());
    }
    #[test]
    fn integer32_exact_load_does_not_grant_binary32_precision_through_a_cross_block_edge() {
        let blocks = [
            block(
                0x1000,
                &[fp(X87Operation::Load(X87Format::Integer32)), jump(0x1100)],
            ),
            block(0x1100, &[Operation::Nop]),
        ];
        let region = Region::from_blocks(&blocks, 0x1000).unwrap();
        let incoming = entries(&region)[1];
        assert!(incoming.native[0] && incoming.bounded[0]);
        assert!(
            !incoming.native24[0] && !incoming.native24_in_single[0],
            "FILD32 preserves 16777217 even at PC24; only a later arithmetic/store may round it"
        );
        assert_ne!(16_777_217_i32, (16_777_217_i32 as f32) as i32);
        assert_eq!(incoming.tags[0], Some(true));
    }
    #[test]
    fn binary64_compare_pop_preserves_exact_ghost_facts_without_arithmetic_promises() {
        let blocks = [
            block(
                0x1000,
                &[fp(X87Operation::Load(X87Format::Float64)), jump(0x1100)],
            ),
            block(
                0x1100,
                &[
                    fp(X87Operation::Compare {
                        index: 0,
                        quiet: false,
                        pop: 1,
                        integer_flags: false,
                    }),
                    jump(0x1200),
                ],
            ),
            block(0x1200, &[Operation::Nop]),
        ];
        let region = Region::from_blocks(&blocks, 0x1000).unwrap();
        let facts = entries(&region);
        assert!(facts[1].native[0]);
        assert!(!facts[1].bounded[0] && !facts[1].native24[0] && !facts[1].native24_in_single[0]);
        assert_eq!(facts[1].tags[0], Some(true));
        assert!(facts[2].native[7]);
        assert!(!facts[2].bounded[7] && !facts[2].native24[7] && !facts[2].native24_in_single[7]);
        assert_eq!(facts[2].tags[7], Some(false));
        assert!(!facts[1].control_admitted && !facts[2].control_admitted);
    }
}
