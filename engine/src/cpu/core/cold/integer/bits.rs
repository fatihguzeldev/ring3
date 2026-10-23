use super::*;

enum Bits {
    Scan(Option<BitScanKind>, Register32, Operand, u32),
    Test(BitTestKind, Operand, BitIndex, u32),
}

fn lower(operation: Operation) -> Option<Bits> {
    use Bits as B;
    Some(match operation {
        Operation::BitScan {
            kind,
            destination,
            source,
        } => B::Scan(Some(kind), destination, reg(source), 4),
        Operation::BitScanWord {
            kind,
            destination,
            source,
        } => B::Scan(Some(kind), destination, reg(source), 2),
        Operation::ReadBitScan {
            kind,
            destination,
            address,
        } => B::Scan(Some(kind), destination, Operand::Memory(address), 4),
        Operation::ReadBitScanWord {
            kind,
            destination,
            address,
        } => B::Scan(Some(kind), destination, Operand::Memory(address), 2),
        Operation::TrailingZeroCount32 {
            destination,
            source,
        } => B::Scan(None, destination, reg(source), 4),
        Operation::ReadTrailingZeroCount32 {
            destination,
            address,
        } => B::Scan(None, destination, Operand::Memory(address), 4),
        Operation::BitTest {
            kind,
            destination,
            index,
        } => B::Test(kind, reg(destination), index, 4),
        Operation::BitTestWord {
            kind,
            destination,
            index,
        } => B::Test(kind, reg(destination), index, 2),
        Operation::ReadBitTest { address, index } => {
            B::Test(BitTestKind::Test, Operand::Memory(address), index, 4)
        }
        Operation::ReadBitTestWord { address, index } => {
            B::Test(BitTestKind::Test, Operand::Memory(address), index, 2)
        }
        Operation::MemoryBitMutation {
            kind,
            address,
            index,
        } => B::Test(kind, Operand::Memory(address), index, 4),
        Operation::MemoryBitMutationWord {
            kind,
            address,
            index,
        } => B::Test(kind, Operand::Memory(address), index, 2),
        Operation::LockedMemoryBitSet32 { address, index } => B::Test(
            BitTestKind::Set,
            Operand::Memory(address),
            BitIndex::Register(index),
            4,
        ),
        Operation::LockedMemoryBitReset32 { address, index } => B::Test(
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
            B::Test(kind, Operand::Memory(address), index, u32::from(width))
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
        Bits::Scan(kind, destination, source, width) => {
            let value = read(source, state, port, width, false)?;
            let result = match kind {
                Some(BitScanKind::Reverse) => 31_u32.wrapping_sub(value.leading_zeros()),
                _ => value.trailing_zeros(),
            };
            if value != 0 || kind.is_none() {
                store(reg(destination), state, port, width, result)?;
            }
            state.eflags = if kind.is_none() {
                (state.eflags & !0x41) | u32::from(value == 0) | (u32::from(result == 0) << 6)
            } else {
                (state.eflags & !0x40) | (u32::from(value == 0) << 6)
            };
        }
        Bits::Test(kind, mut destination, index, width) => {
            let index_value = match index {
                BitIndex::Register(register) => state.registers[register.index()],
                BitIndex::Immediate(value) => u32::from(value),
            };
            if let Operand::Memory(address) = destination {
                let base = offset(address, state);
                let displacement = if matches!(index, BitIndex::Register(_)) {
                    let signed_index = if width == 2 {
                        i32::from(index_value as i16)
                    } else {
                        index_value as i32
                    };
                    base.wrapping_add(
                        ((signed_index >> if width == 2 { 4 } else { 5 }) as u32)
                            .wrapping_mul(width),
                    )
                } else {
                    base
                };
                destination = Operand::Memory(EffectiveAddress {
                    displacement,
                    segment: Some(address.resolved_segment()),
                    address_size: address.address_size,
                    ..Default::default()
                });
            }
            let writes = kind != BitTestKind::Test;
            let old = read(destination, state, port, width, writes)?;
            let bit = index_value & (width * 8 - 1);
            let result = if writes {
                let atomic = match kind {
                    BitTestKind::Set => AtomicKind::BitSet,
                    BitTestKind::Reset => AtomicKind::BitReset,
                    _ => AtomicKind::BitComplement,
                };
                let result = atomic::evaluate(atomic, width, old, bit, state.eflags)
                    .ok_or(Failure::Infrastructure)?;
                store(destination, state, port, width, result.value)?;
                result.flags
            } else {
                (state.eflags & !1) | ((old >> bit) & 1)
            };
            state.eflags = result;
        }
    }
    Ok(true)
}
