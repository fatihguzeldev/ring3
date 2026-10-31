use super::*;
use crate::cpu::x86::atomic::AtomicKind;

#[derive(Clone, Copy)]
enum Target {
    Register(Register32),
    Memory(EffectiveAddress),
}

struct BitOperation {
    kind: BitTestKind,
    bits: u32,
    target: Target,
    index: BitIndex,
}

fn lower(operation: Operation) -> Option<BitOperation> {
    let (kind, bits, target, index) = match operation {
        Operation::BitTest {
            kind,
            destination,
            index,
        } => (kind, 32, Target::Register(destination), index),
        Operation::BitTestWord {
            kind,
            destination,
            index,
        } => (kind, 16, Target::Register(destination), index),
        Operation::ReadBitTest { address, index } => {
            (BitTestKind::Test, 32, Target::Memory(address), index)
        }
        Operation::ReadBitTestWord { address, index } => {
            (BitTestKind::Test, 16, Target::Memory(address), index)
        }
        Operation::MemoryBitMutation {
            kind,
            address,
            index,
        } => (kind, 32, Target::Memory(address), index),
        Operation::MemoryBitMutationWord {
            kind,
            address,
            index,
        } => (kind, 16, Target::Memory(address), index),
        Operation::LockedMemoryBitSet32 { address, index } => (
            BitTestKind::Set,
            32,
            Target::Memory(address),
            BitIndex::Register(index),
        ),
        Operation::LockedMemoryBitReset32 { address, index } => (
            BitTestKind::Reset,
            32,
            Target::Memory(address),
            BitIndex::Register(index),
        ),
        Operation::AtomicMemory {
            kind,
            width: width @ (2 | 4),
            address,
            source,
        } => {
            let kind = match kind {
                AtomicKind::BitSet => BitTestKind::Set,
                AtomicKind::BitReset => BitTestKind::Reset,
                AtomicKind::BitComplement => BitTestKind::Complement,
                _ => return None,
            };
            let index = match source {
                AtomicSource::Register(register) => BitIndex::Register(register),
                AtomicSource::Immediate(value) => BitIndex::Immediate(value as u8),
                _ => return None,
            };
            (kind, u32::from(width) * 8, Target::Memory(address), index)
        }
        _ => return None,
    };
    Some(BitOperation {
        kind,
        bits,
        target,
        index,
    })
}

pub(super) fn supports(operation: &Operation) -> bool {
    lower(*operation).is_some()
}

pub(super) fn emit(out: &mut Output, operation: Operation) -> bool {
    let Some(operation) = lower(operation) else {
        return false;
    };
    let BitOperation {
        kind,
        bits,
        target,
        index,
    } = operation;
    match index {
        BitIndex::Register(register) => {
            out.c().local_get(reg(register)).local_set(B);
        }
        BitIndex::Immediate(value) => {
            out.c().i32_const(i32::from(value)).local_set(B);
        }
    }
    match target {
        Target::Register(register) => {
            out.c().local_get(reg(register));
            if bits == 16 {
                out.c().i32_const(0xffff).i32_and();
            }
            out.c().local_set(A);
        }
        Target::Memory(address) => {
            out.bit_address(address, bits, matches!(index, BitIndex::Register(_)));
            out.read(
                bits / 8 | if kind == BitTestKind::Test { 0 } else { 0x100 },
                A,
            );
        }
    }
    out.c()
        .local_get(A)
        .local_get(B)
        .i32_const((bits - 1) as i32)
        .i32_and()
        .i32_shr_u()
        .i32_const(1)
        .i32_and()
        .local_set(CARRY);
    if kind != BitTestKind::Test {
        out.c()
            .local_get(A)
            .i32_const(1)
            .local_get(B)
            .i32_const((bits - 1) as i32)
            .i32_and()
            .i32_shl();
        match kind {
            BitTestKind::Set => {
                out.c().i32_or();
            }
            BitTestKind::Reset => {
                out.c().i32_const(-1).i32_xor().i32_and();
            }
            BitTestKind::Complement => {
                out.c().i32_xor();
            }
            BitTestKind::Test => unreachable!(),
        }
        out.c().local_set(R);
        match target {
            Target::Register(register) => {
                if bits == 16 {
                    out.store_word(register, R);
                } else {
                    out.c().local_get(R).local_set(reg(register));
                }
            }
            Target::Memory(_) => out.write(bits / 8, R),
        }
    }
    out.c()
        .local_get(FLAGS)
        .i32_wrap_i64()
        .i32_const(!1)
        .i32_and()
        .local_get(CARRY)
        .i32_or()
        .i64_extend_i32_u()
        .local_set(FLAGS);
    true
}

impl Output {
    fn bit_address(&mut self, address: EffectiveAddress, bits: u32, dynamic: bool) {
        self.offset(address);
        if dynamic {
            self.c().local_get(B);
            if bits == 16 {
                self.c().i32_const(16).i32_shl().i32_const(16).i32_shr_s();
            }
            self.c()
                .i32_const(if bits == 16 { 4 } else { 5 })
                .i32_shr_s()
                .i32_const((bits / 8) as i32)
                .i32_mul()
                .i32_add();
            if address.address_size == AddressSize::Bits16 {
                self.c().i32_const(0xffff).i32_and();
            }
        }
        self.c().local_tee(OFFSET);
        match address.resolved_segment() {
            SegmentRegister::Fs => {
                self.c().local_get(0).i32_load(mem(116)).i32_add();
            }
            SegmentRegister::Gs => {
                self.c().local_get(0).i32_load(mem(120)).i32_add();
            }
            _ => {}
        }
        self.c()
            .local_set(ADDRESS)
            .i32_const(address.resolved_segment().index() as i32)
            .local_set(SEGMENT);
    }
}
