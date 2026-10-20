use super::super::{A, ADDRESS, B, CTX, FLAGS, KIND, LOGICAL_ADDRESS, Output, RESULT, mem};
use super::{Operand, byte, reg};
use crate::cpu::x86::{Register32, SegmentRegister, ir::*};

pub(super) fn supports(operation: &Operation) -> bool {
    matches!(
        operation,
        Operation::Extend { .. }
            | Operation::ExtendByteToWord { .. }
            | Operation::ReadExtendByteToWord { .. }
            | Operation::SignExtendAccumulatorWord
            | Operation::SignExtendHighWord
            | Operation::SignExtendHigh
            | Operation::ByteSwap { .. }
            | Operation::FlagsToAh
            | Operation::AhToFlags
            | Operation::SetAlFromCarry
            | Operation::TranslateByte
            | Operation::TranslateByteAddress { .. }
            | Operation::MemoryExchange { .. }
            | Operation::MemoryExchangeByte { .. }
            | Operation::MemoryExchangeWord { .. }
    )
}

pub(super) fn emit(out: &mut Output<'_>, operation: Operation) -> bool {
    match operation {
        Operation::Extend {
            kind,
            destination,
            source,
        } => {
            let (source, width) = match source {
                SmallSource::Register {
                    register,
                    width,
                    high_byte,
                } => (
                    Operand::Register(register.index() as u32 + 2, if high_byte { 8 } else { 0 }),
                    match width {
                        SmallWidth::Byte => 1,
                        SmallWidth::Word => 2,
                    },
                ),
                SmallSource::Memory { address, width } => (
                    Operand::Memory(address),
                    match width {
                        SmallWidth::Byte => 1,
                        SmallWidth::Word => 2,
                    },
                ),
            };
            extend(out, source, width, kind);
            out.store(reg(destination), 4, RESULT);
        }
        Operation::ExtendByteToWord {
            kind,
            destination,
            source,
        } => {
            extend(out, byte(source), 1, kind);
            out.store(reg(destination), 2, RESULT);
        }
        Operation::ReadExtendByteToWord {
            kind,
            destination,
            address,
        } => {
            extend(out, Operand::Memory(address), 1, kind);
            out.store(reg(destination), 2, RESULT);
        }
        Operation::SignExtendAccumulatorWord => {
            extend(out, byte(ByteRegister::Al), 1, ExtensionKind::Sign);
            out.store(reg(Register32::Eax), 2, RESULT);
        }
        Operation::SignExtendHighWord => {
            out.c()
                .local_get(2)
                .i32_const(16)
                .i32_shl()
                .i32_const(31)
                .i32_shr_s()
                .local_set(RESULT);
            out.store(reg(Register32::Edx), 2, RESULT);
        }
        Operation::SignExtendHigh => {
            out.c().local_get(2).i32_const(31).i32_shr_s().local_set(4);
        }
        Operation::ByteSwap { destination } => {
            out.operand(reg(destination), 4, A, false);
            out.c()
                .local_get(A)
                .i32_const(8)
                .i32_rotl()
                .i32_const(0x00ff_00ff)
                .i32_and()
                .local_get(A)
                .i32_const(8)
                .i32_rotr()
                .i32_const(0xff00_ff00_u32 as i32)
                .i32_and()
                .i32_or()
                .local_set(RESULT);
            out.store(reg(destination), 4, RESULT);
        }
        Operation::FlagsToAh => {
            out.materialize(0xd5);
            out.c()
                .local_get(FLAGS)
                .i32_const(0xd5)
                .i32_and()
                .i32_const(2)
                .i32_or()
                .local_set(RESULT);
            out.store(byte(ByteRegister::Ah), 1, RESULT);
        }
        Operation::AhToFlags => {
            out.materialize(super::super::super::flags::ARITHMETIC);
            out.c().i32_const(0).local_set(KIND);
            out.operand(byte(ByteRegister::Ah), 1, A, false);
            out.c()
                .local_get(FLAGS)
                .i32_const(!0xd5)
                .i32_and()
                .local_get(A)
                .i32_const(0xd5)
                .i32_and()
                .i32_or()
                .i32_const(2)
                .i32_or()
                .local_set(FLAGS);
        }
        Operation::SetAlFromCarry => {
            out.materialize(1);
            out.c()
                .local_get(FLAGS)
                .i32_const(1)
                .i32_and()
                .i32_const(-1)
                .i32_mul()
                .local_set(RESULT);
            out.store(byte(ByteRegister::Al), 1, RESULT);
        }
        Operation::TranslateByte | Operation::TranslateByteAddress { .. } => {
            let (address_size, segment) = match operation {
                Operation::TranslateByteAddress { address } => {
                    (address.address_size, address.resolved_segment())
                }
                _ => (AddressSize::Bits32, SegmentRegister::Ds),
            };
            out.c()
                .local_get(5)
                .local_get(2)
                .i32_const(0xff)
                .i32_and()
                .i32_add();
            if address_size == AddressSize::Bits16 {
                out.c().i32_const(0xffff).i32_and();
            }
            out.c().local_tee(LOGICAL_ADDRESS);
            match segment {
                SegmentRegister::Fs => {
                    out.c().local_get(CTX).i32_load(mem(104, 2)).i32_add();
                }
                SegmentRegister::Gs => {
                    out.c().local_get(CTX).i32_load(mem(108, 2)).i32_add();
                }
                _ => {}
            }
            out.c().local_set(ADDRESS);
            out.read(1, RESULT);
            out.store(byte(ByteRegister::Al), 1, RESULT);
        }
        Operation::MemoryExchange { address, source } => exchange(out, address, reg(source), 4),
        Operation::MemoryExchangeWord { address, source } => exchange(out, address, reg(source), 2),
        Operation::MemoryExchangeByte { address, source } => {
            exchange(out, address, byte(source), 1)
        }
        _ => return false,
    }
    true
}

fn extend(out: &mut Output<'_>, source: Operand, width: u32, kind: ExtensionKind) {
    out.operand(source, width, RESULT, false);
    if kind == ExtensionKind::Sign {
        let shift = (32 - width * 8) as i32;
        out.c()
            .local_get(RESULT)
            .i32_const(shift)
            .i32_shl()
            .i32_const(shift)
            .i32_shr_s()
            .local_set(RESULT);
    }
}

fn exchange(out: &mut Output<'_>, address: EffectiveAddress, source: Operand, width: u32) {
    out.operand(Operand::Memory(address), width, A, true);
    out.operand(source, width, B, false);
    out.store(Operand::Memory(address), width, B);
    out.store(source, width, A);
}
