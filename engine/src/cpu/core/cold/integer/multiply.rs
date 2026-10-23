use super::extended::signed;
use super::*;

enum Multiply {
    Accumulate(MultiplyKind, Operand, u32),
    Signed(Register32, Operand, Option<u32>, u32),
    Divide(DivideKind, Operand, u32),
}

fn lower(operation: Operation) -> Option<Multiply> {
    use Multiply as M;
    Some(match operation {
        Operation::SignedMultiply {
            destination,
            source,
            immediate,
        } => M::Signed(destination, location(source), immediate, 4),
        Operation::SignedMultiplyWord {
            destination,
            source,
        } => M::Signed(destination, reg(source), None, 2),
        Operation::ReadSignedMultiplyWord {
            destination,
            address,
        } => M::Signed(destination, Operand::Memory(address), None, 2),
        Operation::SignedMultiplyImmediateWord {
            destination,
            source,
            immediate,
        } => M::Signed(destination, reg(source), Some(u32::from(immediate)), 2),
        Operation::ReadSignedMultiplyImmediateWord {
            destination,
            address,
            immediate,
        } => M::Signed(
            destination,
            Operand::Memory(address),
            Some(u32::from(immediate)),
            2,
        ),
        Operation::MultiplyAccumulator { kind, source } => M::Accumulate(kind, reg(source), 4),
        Operation::WordMultiplyAccumulator { kind, source } => M::Accumulate(kind, reg(source), 2),
        Operation::ByteMultiplyAccumulator { kind, source } => M::Accumulate(kind, byte(source), 1),
        Operation::ReadMultiplyAccumulator { kind, address } => {
            M::Accumulate(kind, Operand::Memory(address), 4)
        }
        Operation::ReadWordMultiplyAccumulator { kind, address } => {
            M::Accumulate(kind, Operand::Memory(address), 2)
        }
        Operation::ReadByteMultiplyAccumulator { kind, address } => {
            M::Accumulate(kind, Operand::Memory(address), 1)
        }
        Operation::DivideAccumulator { kind, source } => M::Divide(kind, reg(source), 4),
        Operation::WordDivideAccumulator { kind, source } => M::Divide(kind, reg(source), 2),
        Operation::ByteDivideAccumulator { kind, source } => M::Divide(kind, byte(source), 1),
        Operation::ReadDivideAccumulator { kind, address } => {
            M::Divide(kind, Operand::Memory(address), 4)
        }
        Operation::ReadWordDivideAccumulator { kind, address } => {
            M::Divide(kind, Operand::Memory(address), 2)
        }
        Operation::ReadByteDivideAccumulator { kind, address } => {
            M::Divide(kind, Operand::Memory(address), 1)
        }
        _ => return None,
    })
}

pub(super) fn execute(
    operation: Operation,
    state: &mut State32,
    port: &mut Port<'_>,
) -> Result<bool, Failure> {
    let Some(operation) = lower(operation) else {
        return Ok(false);
    };
    match operation {
        Multiply::Signed(destination, source, immediate, width) => {
            let right = read(source, state, port, width, false)?;
            let left = match immediate {
                Some(value) => value & mask(width),
                None => read(reg(destination), state, port, width, false)?,
            };
            let product = signed(left, width) * signed(right, width);
            let result = product as u32 & mask(width);
            store(reg(destination), state, port, width, result)?;
            flags(state, product != signed(result, width));
        }
        Multiply::Accumulate(kind, source, width) => {
            let right = read(source, state, port, width, false)?;
            let left = read(reg(Register32::Eax), state, port, width, false)?;
            let product = if kind == MultiplyKind::Signed {
                (signed(left, width) * signed(right, width)) as u64
            } else {
                u64::from(left) * u64::from(right)
            };
            let low = product as u32 & mask(width);
            let overflow = if kind == MultiplyKind::Signed {
                product as i64 != signed(low, width)
            } else {
                product > u64::from(mask(width))
            };
            store(
                reg(Register32::Eax),
                state,
                port,
                if width == 1 { 2 } else { width },
                product as u32,
            )?;
            if width != 1 {
                store(
                    reg(Register32::Edx),
                    state,
                    port,
                    width,
                    (product >> (width * 8)) as u32,
                )?;
            }
            flags(state, overflow);
        }
        Multiply::Divide(kind, source, width) => {
            let denominator = read(source, state, port, width, false)?;
            if denominator == 0 {
                return Err(Failure::CpuFault(0));
            }
            let raw = match width {
                1 => u64::from(state.registers[0] & 0xffff),
                2 => u64::from((state.registers[2] << 16) | (state.registers[0] & 0xffff)),
                _ => u64::from(state.registers[2]) << 32 | u64::from(state.registers[0]),
            };
            let (quotient, remainder) = if kind == DivideKind::Unsigned {
                let denominator = u64::from(denominator);
                let quotient = raw / denominator;
                if quotient > u64::from(mask(width)) {
                    return Err(Failure::CpuFault(0));
                }
                (quotient as u32, (raw % denominator) as u32)
            } else {
                let numerator = match width {
                    1 => i64::from(raw as i16),
                    2 => i64::from(raw as i32),
                    _ => raw as i64,
                };
                let denominator = signed(denominator, width);
                let Some(quotient) = numerator.checked_div(denominator) else {
                    return Err(Failure::CpuFault(0));
                };
                let limit = 1_i64 << (width * 8 - 1);
                if quotient < -limit || quotient >= limit {
                    return Err(Failure::CpuFault(0));
                }
                (quotient as u32, (numerator % denominator) as u32)
            };
            if width == 1 {
                store(
                    reg(Register32::Eax),
                    state,
                    port,
                    2,
                    (quotient & 0xff) | (remainder & 0xff) << 8,
                )?;
            } else {
                store(reg(Register32::Eax), state, port, width, quotient)?;
                store(reg(Register32::Edx), state, port, width, remainder)?;
            }
        }
    }
    Ok(true)
}

fn flags(state: &mut State32, overflow: bool) {
    state.eflags = (state.eflags & !0x8d5) | u32::from(overflow) * 0x801;
}
