use super::memory::{Failure, Port, offset};
use crate::cpu::x86::{
    Register32, State32,
    atomic::{self, AtomicKind},
    ir::*,
};

#[derive(Clone, Copy)]
pub(super) enum Operand {
    Register(usize, u32),
    Memory(EffectiveAddress),
    Immediate(u32),
}

enum Integer {
    Move(Operand, Operand, u32),
    Binary(AtomicKind, Operand, Operand, u32, bool),
    Unary(AtomicKind, Operand, u32),
    Lea(Register32, EffectiveAddress, u32),
    Set(Condition, Operand),
    ConditionalMove(Condition, Operand, Operand, u32),
    Exchange(Operand, Operand, u32),
    Carry(CarryKind),
    Direction(bool),
}

pub(super) fn reg(register: Register32) -> Operand {
    Operand::Register(register.index(), 0)
}
fn byte(register: ByteRegister) -> Operand {
    let (index, shift) = match register {
        ByteRegister::Al => (0, 0),
        ByteRegister::Cl => (1, 0),
        ByteRegister::Dl => (2, 0),
        ByteRegister::Bl => (3, 0),
        ByteRegister::Ah => (0, 8),
        ByteRegister::Ch => (1, 8),
        ByteRegister::Dh => (2, 8),
        ByteRegister::Bh => (3, 8),
    };
    Operand::Register(index, shift)
}
pub(super) fn location(location: Location32) -> Operand {
    match location {
        Location32::Register(register) => reg(register),
        Location32::Memory(address) => Operand::Memory(address),
    }
}
pub(super) fn value(value: Value32) -> Operand {
    match value {
        Value32::Register(register) => reg(register),
        Value32::Memory(address) => Operand::Memory(address),
        Value32::Immediate(value) => Operand::Immediate(value),
    }
}
pub(super) fn word(value: WordValue) -> Operand {
    match value {
        WordValue::Register(register) => reg(register),
        WordValue::Immediate(value) => Operand::Immediate(u32::from(value)),
    }
}
fn byte_value(value: ByteValue) -> Operand {
    match value {
        ByteValue::Register(register) => byte(register),
        ByteValue::Immediate(value) => Operand::Immediate(u32::from(value)),
    }
}
pub(super) fn mask(width: u32) -> u32 {
    u32::MAX >> (32 - width * 8)
}

pub(super) fn read(
    operand: Operand,
    state: &State32,
    port: &Port<'_>,
    width: u32,
    modify: bool,
) -> Result<u32, Failure> {
    Ok(match operand {
        Operand::Register(index, shift) => (state.registers[index] >> shift) & mask(width),
        Operand::Immediate(value) => value & mask(width),
        Operand::Memory(address) => port.read(address, state, width, modify)?,
    })
}
pub(super) fn store(
    operand: Operand,
    state: &mut State32,
    port: &mut Port<'_>,
    width: u32,
    value: u32,
) -> Result<(), Failure> {
    match operand {
        Operand::Register(index, shift) => {
            let bits = mask(width) << shift;
            state.registers[index] = (state.registers[index] & !bits) | ((value << shift) & bits);
        }
        Operand::Memory(address) => port.write(address, state, width, value)?,
        Operand::Immediate(_) => return Err(Failure::Infrastructure),
    }
    Ok(())
}

pub(super) fn condition(condition: Condition, flags: u32) -> bool {
    let cf = flags & 1 != 0;
    let pf = flags & 4 != 0;
    let zf = flags & 0x40 != 0;
    let sf = flags & 0x80 != 0;
    let of = flags & 0x800 != 0;
    match condition {
        Condition::Overflow => of,
        Condition::NotOverflow => !of,
        Condition::Below => cf,
        Condition::AboveOrEqual => !cf,
        Condition::Equal => zf,
        Condition::NotEqual => !zf,
        Condition::BelowOrEqual => cf || zf,
        Condition::Above => !cf && !zf,
        Condition::Sign => sf,
        Condition::NotSign => !sf,
        Condition::Parity => pf,
        Condition::NotParity => !pf,
        Condition::Less => sf != of,
        Condition::GreaterOrEqual => sf == of,
        Condition::LessOrEqual => zf || sf != of,
        Condition::Greater => !zf && sf == of,
    }
}

fn binary(kind: BinaryKind, destination: Operand, source: Operand, width: u32) -> Integer {
    let (kind, writes) = match kind {
        BinaryKind::Add => (AtomicKind::Add, true),
        BinaryKind::Adc => (AtomicKind::AddCarry, true),
        BinaryKind::Sub => (AtomicKind::Subtract, true),
        BinaryKind::Sbb => (AtomicKind::SubtractBorrow, true),
        BinaryKind::Cmp => (AtomicKind::Subtract, false),
        BinaryKind::Test => (AtomicKind::And, false),
        BinaryKind::And => (AtomicKind::And, true),
        BinaryKind::Or => (AtomicKind::Or, true),
        BinaryKind::Xor => (AtomicKind::Xor, true),
    };
    Integer::Binary(kind, destination, source, width, writes)
}
fn unary(kind: UnaryKind, destination: Operand, width: u32) -> Integer {
    Integer::Unary(
        match kind {
            UnaryKind::Inc => AtomicKind::Increment,
            UnaryKind::Dec => AtomicKind::Decrement,
            UnaryKind::Neg => AtomicKind::Negate,
            UnaryKind::Not => AtomicKind::Not,
        },
        destination,
        width,
    )
}

fn lower(operation: Operation) -> Option<Integer> {
    use Integer as I;
    Some(match operation {
        Operation::Move {
            destination,
            source,
        } => I::Move(location(destination), value(source), 4),
        Operation::MoveByte {
            destination,
            source,
        } => I::Move(byte(destination), byte_value(source), 1),
        Operation::MoveWord {
            destination,
            source,
        } => I::Move(reg(destination), word(source), 2),
        Operation::LoadByte {
            destination,
            address,
        } => I::Move(byte(destination), Operand::Memory(address), 1),
        Operation::StoreByte { address, source } => {
            I::Move(Operand::Memory(address), byte_value(source), 1)
        }
        Operation::LoadWord {
            destination,
            address,
        } => I::Move(reg(destination), Operand::Memory(address), 2),
        Operation::StoreWord { address, source } => {
            I::Move(Operand::Memory(address), word(source), 2)
        }
        Operation::Binary {
            kind,
            destination,
            source,
        } => binary(kind, location(destination), value(source), 4),
        Operation::ArithmeticWord {
            kind,
            destination,
            source,
        } => binary(
            match kind {
                WordArithmeticKind::Add => BinaryKind::Add,
                WordArithmeticKind::Sub => BinaryKind::Sub,
                WordArithmeticKind::Adc => BinaryKind::Adc,
                WordArithmeticKind::Sbb => BinaryKind::Sbb,
            },
            reg(destination),
            word(source),
            2,
        ),
        Operation::ArithmeticByte {
            kind,
            destination,
            source,
        } => binary(
            match kind {
                ByteArithmeticKind::Add => BinaryKind::Add,
                ByteArithmeticKind::Sub => BinaryKind::Sub,
                ByteArithmeticKind::Adc => BinaryKind::Adc,
                ByteArithmeticKind::Sbb => BinaryKind::Sbb,
            },
            byte(destination),
            byte_value(source),
            1,
        ),
        Operation::LogicalWord {
            kind,
            destination,
            source,
        } => binary(
            match kind {
                WordLogicalKind::And => BinaryKind::And,
                WordLogicalKind::Or => BinaryKind::Or,
                WordLogicalKind::Xor => BinaryKind::Xor,
            },
            reg(destination),
            word(source),
            2,
        ),
        Operation::LogicalByte {
            kind,
            destination,
            source,
        } => binary(
            match kind {
                ByteLogicalKind::And => BinaryKind::And,
                ByteLogicalKind::Or => BinaryKind::Or,
                ByteLogicalKind::Xor => BinaryKind::Xor,
            },
            byte(destination),
            byte_value(source),
            1,
        ),
        Operation::ReadArithmeticWord {
            kind,
            destination,
            address,
        } => binary(
            match kind {
                WordReadArithmeticKind::Add => BinaryKind::Add,
                WordReadArithmeticKind::Sub => BinaryKind::Sub,
                WordReadArithmeticKind::Adc => BinaryKind::Adc,
                WordReadArithmeticKind::Sbb => BinaryKind::Sbb,
            },
            reg(destination),
            Operand::Memory(address),
            2,
        ),
        Operation::ReadArithmeticByte {
            kind,
            destination,
            address,
        } => binary(
            match kind {
                ByteReadArithmeticKind::Add => BinaryKind::Add,
                ByteReadArithmeticKind::Sub => BinaryKind::Sub,
                ByteReadArithmeticKind::Adc => BinaryKind::Adc,
                ByteReadArithmeticKind::Sbb => BinaryKind::Sbb,
            },
            byte(destination),
            Operand::Memory(address),
            1,
        ),
        Operation::MemoryArithmeticWord {
            kind,
            address,
            source,
        } => binary(
            match kind {
                WordMemoryArithmeticKind::Add => BinaryKind::Add,
                WordMemoryArithmeticKind::Sub => BinaryKind::Sub,
                WordMemoryArithmeticKind::Adc => BinaryKind::Adc,
                WordMemoryArithmeticKind::Sbb => BinaryKind::Sbb,
            },
            Operand::Memory(address),
            word(source),
            2,
        ),
        Operation::MemoryArithmeticByte {
            kind,
            address,
            source,
        } => binary(
            match kind {
                MemoryByteArithmeticKind::Add => BinaryKind::Add,
                MemoryByteArithmeticKind::Sub => BinaryKind::Sub,
                MemoryByteArithmeticKind::Adc => BinaryKind::Adc,
                MemoryByteArithmeticKind::Sbb => BinaryKind::Sbb,
            },
            Operand::Memory(address),
            byte_value(source),
            1,
        ),
        Operation::ReadLogicalWord {
            kind,
            destination,
            address,
        } => binary(
            match kind {
                WordLogicalKind::And => BinaryKind::And,
                WordLogicalKind::Or => BinaryKind::Or,
                WordLogicalKind::Xor => BinaryKind::Xor,
            },
            reg(destination),
            Operand::Memory(address),
            2,
        ),
        Operation::ReadLogicalByte {
            kind,
            destination,
            address,
        } => binary(
            match kind {
                ByteLogicalKind::And => BinaryKind::And,
                ByteLogicalKind::Or => BinaryKind::Or,
                ByteLogicalKind::Xor => BinaryKind::Xor,
            },
            byte(destination),
            Operand::Memory(address),
            1,
        ),
        Operation::MemoryLogicalWord {
            kind,
            address,
            source,
        } => binary(
            match kind {
                WordLogicalKind::And => BinaryKind::And,
                WordLogicalKind::Or => BinaryKind::Or,
                WordLogicalKind::Xor => BinaryKind::Xor,
            },
            Operand::Memory(address),
            word(source),
            2,
        ),
        Operation::MemoryLogicalByte {
            kind,
            address,
            source,
        } => binary(
            match kind {
                ByteLogicalKind::And => BinaryKind::And,
                ByteLogicalKind::Or => BinaryKind::Or,
                ByteLogicalKind::Xor => BinaryKind::Xor,
            },
            Operand::Memory(address),
            byte_value(source),
            1,
        ),
        Operation::CompareWord { left, right } => {
            binary(BinaryKind::Cmp, reg(left), word(right), 2)
        }
        Operation::TestWord { left, right } => binary(BinaryKind::Test, reg(left), word(right), 2),
        Operation::CompareByte { left, right } => {
            binary(BinaryKind::Cmp, byte(left), byte_value(right), 1)
        }
        Operation::TestByte { left, right } => {
            binary(BinaryKind::Test, byte(left), byte_value(right), 1)
        }
        Operation::ReadCompareWord { left, address } => {
            binary(BinaryKind::Cmp, reg(left), Operand::Memory(address), 2)
        }
        Operation::ReadCompareByte { left, address } => {
            binary(BinaryKind::Cmp, byte(left), Operand::Memory(address), 1)
        }
        Operation::MemoryCompareWord { address, right } => {
            binary(BinaryKind::Cmp, Operand::Memory(address), word(right), 2)
        }
        Operation::MemoryTestWord { address, right } => {
            binary(BinaryKind::Test, Operand::Memory(address), word(right), 2)
        }
        Operation::MemoryPredicateByte {
            kind,
            address,
            right,
        } => binary(
            match kind {
                BytePredicateKind::Cmp => BinaryKind::Cmp,
                BytePredicateKind::Test => BinaryKind::Test,
            },
            Operand::Memory(address),
            byte_value(right),
            1,
        ),
        Operation::Unary { kind, destination } => unary(kind, location(destination), 4),
        Operation::UnaryWord { kind, destination } => unary(kind, reg(destination), 2),
        Operation::UnaryByte { kind, destination } => unary(kind, byte(destination), 1),
        Operation::MemoryUnaryWord { kind, address } => unary(kind, Operand::Memory(address), 2),
        Operation::MemoryUnaryByte { kind, address } => unary(kind, Operand::Memory(address), 1),
        Operation::Lea {
            destination,
            address,
        } => I::Lea(destination, address, 4),
        Operation::LeaWord {
            destination,
            address,
        } => I::Lea(destination, address, 2),
        Operation::SetByte {
            condition,
            destination,
        } => I::Set(condition, byte(destination)),
        Operation::MemorySetByte { condition, address } => {
            I::Set(condition, Operand::Memory(address))
        }
        Operation::ConditionalMove {
            condition,
            destination,
            source,
        } => I::ConditionalMove(condition, reg(destination), reg(source), 4),
        Operation::ConditionalMoveWord {
            condition,
            destination,
            source,
        } => I::ConditionalMove(condition, reg(destination), reg(source), 2),
        Operation::ReadConditionalMove {
            condition,
            destination,
            address,
        } => I::ConditionalMove(condition, reg(destination), Operand::Memory(address), 4),
        Operation::ReadConditionalMoveWord {
            condition,
            destination,
            address,
        } => I::ConditionalMove(condition, reg(destination), Operand::Memory(address), 2),
        Operation::Exchange { left, right } => I::Exchange(reg(left), reg(right), 4),
        Operation::ExchangeWord { left, right } => I::Exchange(reg(left), reg(right), 2),
        Operation::ExchangeByte { left, right } => I::Exchange(byte(left), byte(right), 1),
        Operation::MemoryExchange { address, source } => {
            I::Exchange(reg(source), Operand::Memory(address), 4)
        }
        Operation::MemoryExchangeWord { address, source } => {
            I::Exchange(reg(source), Operand::Memory(address), 2)
        }
        Operation::MemoryExchangeByte { address, source } => {
            I::Exchange(byte(source), Operand::Memory(address), 1)
        }
        Operation::Carry { kind } => I::Carry(kind),
        Operation::Direction { set } => I::Direction(set),
        Operation::AtomicMemory {
            kind,
            width,
            address,
            source,
        } if matches!(width, 1 | 2 | 4) => {
            let source = match source {
                AtomicSource::Register(register) => reg(register),
                AtomicSource::ByteRegister(register) => byte(register),
                AtomicSource::Immediate(value) => Operand::Immediate(value),
            };
            match kind {
                AtomicKind::BitSet | AtomicKind::BitReset | AtomicKind::BitComplement => {
                    return None;
                }
                AtomicKind::Increment
                | AtomicKind::Decrement
                | AtomicKind::Negate
                | AtomicKind::Not => I::Unary(kind, Operand::Memory(address), u32::from(width)),
                _ => I::Binary(
                    kind,
                    Operand::Memory(address),
                    source,
                    u32::from(width),
                    true,
                ),
            }
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
        Integer::Move(destination, source, width) => {
            let value = read(source, state, port, width, false)?;
            store(destination, state, port, width, value)?;
        }
        Integer::Binary(kind, destination, source, width, writes) => {
            let left = read(destination, state, port, width, writes)?;
            let right = read(source, state, port, width, false)?;
            let result = atomic::evaluate(kind, width, left, right, state.eflags)
                .ok_or(Failure::Infrastructure)?;
            if writes {
                store(destination, state, port, width, result.value)?;
            }
            state.eflags = result.flags;
        }
        Integer::Unary(kind, destination, width) => {
            let old = read(destination, state, port, width, true)?;
            let result = atomic::evaluate(kind, width, old, 0, state.eflags)
                .ok_or(Failure::Infrastructure)?;
            store(destination, state, port, width, result.value)?;
            state.eflags = result.flags;
        }
        Integer::Lea(destination, address, width) => {
            let value = offset(address, state);
            store(reg(destination), state, port, width, value)?;
        }
        Integer::Set(test, destination) => {
            let value = u32::from(condition(test, state.eflags));
            store(destination, state, port, 1, value)?;
        }
        Integer::ConditionalMove(test, destination, source, width) => {
            let value = read(source, state, port, width, false)?;
            if condition(test, state.eflags) {
                store(destination, state, port, width, value)?;
            }
        }
        Integer::Exchange(left, right, width) => {
            let a = read(left, state, port, width, true)?;
            let b = read(right, state, port, width, true)?;
            // the only memory operand is stored before the infallible register update.
            store(right, state, port, width, a)?;
            store(left, state, port, width, b)?;
        }
        Integer::Carry(kind) => {
            state.eflags = match kind {
                CarryKind::Clear => state.eflags & !1,
                CarryKind::Set => state.eflags | 1,
                CarryKind::Complement => state.eflags ^ 1,
            }
        }
        Integer::Direction(set) => state.eflags = (state.eflags & !0x400) | (u32::from(set) << 10),
    }
    Ok(true)
}
