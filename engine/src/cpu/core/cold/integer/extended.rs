use super::*;

pub(super) fn execute(
    operation: Operation,
    state: &mut State32,
    port: &mut Port<'_>,
) -> Result<bool, Failure> {
    let extension = match operation {
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
                    Operand::Register(register.index(), if high_byte { 8 } else { 0 }),
                    if width == SmallWidth::Byte { 1 } else { 2 },
                ),
                SmallSource::Memory { address, width } => (
                    Operand::Memory(address),
                    if width == SmallWidth::Byte { 1 } else { 2 },
                ),
            };
            Some((kind, destination, source, width, 4))
        }
        Operation::ExtendByteToWord {
            kind,
            destination,
            source,
        } => Some((kind, destination, byte(source), 1, 2)),
        Operation::ReadExtendByteToWord {
            kind,
            destination,
            address,
        } => Some((kind, destination, Operand::Memory(address), 1, 2)),
        Operation::SignExtendAccumulatorWord => Some((
            ExtensionKind::Sign,
            Register32::Eax,
            byte(ByteRegister::Al),
            1,
            2,
        )),
        _ => None,
    };
    if let Some((kind, destination, source, width, output_width)) = extension {
        let value = read(source, state, port, width, false)?;
        let value = if kind == ExtensionKind::Sign {
            signed(value, width) as u32
        } else {
            value
        };
        store(reg(destination), state, port, output_width, value)?;
        return Ok(true);
    }
    match operation {
        Operation::SignExtendHighWord => {
            let value = (state.registers[0] as i16 >> 15) as u32;
            store(reg(Register32::Edx), state, port, 2, value)?;
        }
        Operation::SignExtendHigh => state.registers[2] = (state.registers[0] as i32 >> 31) as u32,
        Operation::ByteSwap { destination } => {
            state.registers[destination.index()] = state.registers[destination.index()].swap_bytes()
        }
        Operation::FlagsToAh => {
            let value = (state.eflags & 0xd5) | 2;
            store(byte(ByteRegister::Ah), state, port, 1, value)?;
        }
        Operation::AhToFlags => {
            state.eflags = (state.eflags & !0xd5) | ((state.registers[0] >> 8) & 0xd5) | 2
        }
        Operation::SetAlFromCarry => {
            let value = 0_u32.wrapping_sub(state.eflags & 1);
            store(byte(ByteRegister::Al), state, port, 1, value)?;
        }
        Operation::TranslateByte | Operation::TranslateByteAddress { .. } => {
            let (address_size, segment) = match operation {
                Operation::TranslateByteAddress { address } => {
                    (address.address_size, address.resolved_segment())
                }
                _ => (AddressSize::Bits32, crate::cpu::x86::SegmentRegister::Ds),
            };
            let address = EffectiveAddress {
                base: Some(Register32::Ebx),
                displacement: state.registers[0] & 0xff,
                segment: Some(segment),
                address_size,
                ..Default::default()
            };
            let value = port.read(address, state, 1, false)?;
            store(byte(ByteRegister::Al), state, port, 1, value)?;
        }
        _ => return Ok(false),
    }
    Ok(true)
}

pub(super) fn signed(value: u32, width: u32) -> i64 {
    match width {
        1 => i64::from(value as i8),
        2 => i64::from(value as i16),
        _ => i64::from(value as i32),
    }
}
