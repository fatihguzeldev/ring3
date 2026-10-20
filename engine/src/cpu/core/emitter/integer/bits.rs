use super::super::{A, ADDRESS, B, CARRY, FLAGS, KIND, LOGICAL_ADDRESS, Output, RESULT};
use super::{Operand, reg};
use crate::cpu::x86::{Register32, SegmentRegister, atomic::AtomicKind, ir::*};
use wasm_encoder::BlockType;

enum Bits {
    Scan(Option<BitScanKind>, Register32, Operand, u32),
    Test(BitTestKind, Operand, BitIndex, u32),
}

fn lower(operation: Operation) -> Option<Bits> {
    use Bits as I;
    Some(match operation {
        Operation::BitScan {
            kind,
            destination,
            source,
        } => I::Scan(Some(kind), destination, reg(source), 4),
        Operation::BitScanWord {
            kind,
            destination,
            source,
        } => I::Scan(Some(kind), destination, reg(source), 2),
        Operation::ReadBitScan {
            kind,
            destination,
            address,
        } => I::Scan(Some(kind), destination, Operand::Memory(address), 4),
        Operation::ReadBitScanWord {
            kind,
            destination,
            address,
        } => I::Scan(Some(kind), destination, Operand::Memory(address), 2),
        Operation::TrailingZeroCount32 {
            destination,
            source,
        } => I::Scan(None, destination, reg(source), 4),
        Operation::ReadTrailingZeroCount32 {
            destination,
            address,
        } => I::Scan(None, destination, Operand::Memory(address), 4),
        Operation::BitTest {
            kind,
            destination,
            index,
        } => I::Test(kind, reg(destination), index, 4),
        Operation::BitTestWord {
            kind,
            destination,
            index,
        } => I::Test(kind, reg(destination), index, 2),
        Operation::ReadBitTest { address, index } => {
            I::Test(BitTestKind::Test, Operand::Memory(address), index, 4)
        }
        Operation::ReadBitTestWord { address, index } => {
            I::Test(BitTestKind::Test, Operand::Memory(address), index, 2)
        }
        Operation::MemoryBitMutation {
            kind,
            address,
            index,
        } => I::Test(kind, Operand::Memory(address), index, 4),
        Operation::MemoryBitMutationWord {
            kind,
            address,
            index,
        } => I::Test(kind, Operand::Memory(address), index, 2),
        Operation::LockedMemoryBitSet32 { address, index } => I::Test(
            BitTestKind::Set,
            Operand::Memory(address),
            BitIndex::Register(index),
            4,
        ),
        Operation::LockedMemoryBitReset32 { address, index } => I::Test(
            BitTestKind::Reset,
            Operand::Memory(address),
            BitIndex::Register(index),
            4,
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
            I::Test(kind, Operand::Memory(address), index, u32::from(width))
        }
        _ => return None,
    })
}

pub(super) fn supports(operation: &Operation) -> bool {
    lower(*operation).is_some()
}

pub(super) fn emit(out: &mut Output<'_>, operation: Operation) -> bool {
    let Some(operation) = lower(operation) else {
        return false;
    };
    match operation {
        Bits::Scan(kind, destination, source, width) => {
            out.operand(source, width, A, false);
            out.materialize(super::super::super::flags::ARITHMETIC);
            out.c().i32_const(0).local_set(KIND);
            if kind.is_some() {
                out.c().local_get(A).if_(BlockType::Empty);
            }
            match kind {
                Some(BitScanKind::Reverse) => {
                    out.c().i32_const(31).local_get(A).i32_clz().i32_sub();
                }
                Some(BitScanKind::Forward) => {
                    out.c().local_get(A).i32_ctz();
                }
                None => {
                    out.c().local_get(A).i32_ctz();
                }
            }
            out.c().local_set(RESULT);
            out.store(reg(destination), width, RESULT);
            if kind.is_some() {
                out.c().end();
            }
            // undefined arithmetic flags retain their prior value in this profile.
            out.c()
                .local_get(FLAGS)
                .i32_const(if kind.is_none() { !0x41 } else { !0x40 })
                .i32_and();
            if kind.is_none() {
                out.c().local_get(A).i32_eqz().i32_or().local_get(RESULT);
            } else {
                out.c().local_get(A);
            }
            out.c()
                .i32_eqz()
                .i32_const(6)
                .i32_shl()
                .i32_or()
                .local_set(FLAGS);
        }
        Bits::Test(kind, destination, index, width) => {
            match index {
                BitIndex::Register(register) => {
                    out.c().local_get(register.index() as u32 + 2);
                }
                BitIndex::Immediate(value) => {
                    out.c().i32_const(i32::from(value));
                }
            }
            out.c().local_set(B);
            let writes = kind != BitTestKind::Test;
            if let Operand::Memory(address) = destination {
                out.effective_address(address, false);
                if matches!(index, BitIndex::Register(_)) {
                    out.c().local_get(ADDRESS).local_get(B);
                    if width == 2 {
                        out.c().i32_const(16).i32_shl().i32_const(16).i32_shr_s();
                    }
                    out.c()
                        .i32_const(if width == 2 { 4 } else { 5 })
                        .i32_shr_s()
                        .i32_const(width as i32)
                        .i32_mul()
                        .i32_add()
                        .local_set(ADDRESS);
                }
                out.c().local_get(ADDRESS);
                if address.address_size == AddressSize::Bits16 {
                    out.c().i32_const(0xffff).i32_and();
                }
                out.c().local_tee(LOGICAL_ADDRESS);
                let offset = match address.resolved_segment() {
                    SegmentRegister::Fs => Some(104),
                    SegmentRegister::Gs => Some(108),
                    _ => None,
                };
                if let Some(offset) = offset {
                    out.c()
                        .local_get(super::super::CTX)
                        .i32_load(super::super::mem(offset, 2))
                        .i32_add();
                }
                out.c().local_set(ADDRESS);
                out.read(width | if writes { 0x100 } else { 0 }, A);
            } else {
                out.operand(destination, width, A, false);
            }
            out.c()
                .local_get(B)
                .i32_const((width * 8 - 1) as i32)
                .i32_and()
                .local_set(B);
            out.c()
                .local_get(A)
                .local_get(B)
                .i32_shr_u()
                .i32_const(1)
                .i32_and()
                .local_set(CARRY);
            if writes {
                out.c().local_get(A).i32_const(1).local_get(B).i32_shl();
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
                    _ => unreachable!(),
                }
                out.c().local_set(RESULT);
                if matches!(destination, Operand::Memory(_)) {
                    out.write(width, RESULT);
                } else {
                    out.store(destination, width, RESULT);
                }
            }
            out.materialize(super::super::super::flags::ARITHMETIC);
            out.c()
                .i32_const(0)
                .local_set(KIND)
                .local_get(FLAGS)
                .i32_const(!1)
                .i32_and()
                .local_get(CARRY)
                .i32_or()
                .local_set(FLAGS);
        }
    }
    true
}
