use super::super::flags::{self, FlagKind};
use super::{
    A, ADDRESS, B, CARRY, CARRY_IN, FLAGS, KIND, LEFT, Output, RESULT, RIGHT, VALUE, WIDTH,
};
use crate::cpu::x86::{Register32, SegmentRegister, ir::*};

#[derive(Clone, Copy)]
enum Operand {
    Register(u32, u32),
    Memory(EffectiveAddress),
    Immediate(u32),
}

enum Integer {
    Move(Operand, Operand, u32),
    Binary(BinaryKind, Operand, Operand, u32),
    Unary(UnaryKind, Operand, u32),
    Lea(Register32, EffectiveAddress, u32),
    Set(Condition, Operand),
    ConditionalMove(Condition, Operand, Operand, u32),
    Exchange(Operand, Operand, u32),
    Carry(CarryKind),
    Direction(bool),
}

fn reg(register: Register32) -> Operand {
    Operand::Register(register.index() as u32 + 2, 0)
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
    Operand::Register(index + 2, shift)
}
fn location(location: Location32) -> Operand {
    match location {
        Location32::Register(r) => reg(r),
        Location32::Memory(a) => Operand::Memory(a),
    }
}
fn value(value: Value32) -> Operand {
    match value {
        Value32::Register(r) => reg(r),
        Value32::Memory(a) => Operand::Memory(a),
        Value32::Immediate(v) => Operand::Immediate(v),
    }
}
fn word(value: WordValue) -> Operand {
    match value {
        WordValue::Register(r) => reg(r),
        WordValue::Immediate(v) => Operand::Immediate(u32::from(v)),
    }
}
fn byte_value(value: ByteValue) -> Operand {
    match value {
        ByteValue::Register(r) => byte(r),
        ByteValue::Immediate(v) => Operand::Immediate(u32::from(v)),
    }
}

fn lower(operation: Operation) -> Option<Integer> {
    use Integer as I;
    let binary_word = |kind| match kind {
        WordArithmeticKind::Add => BinaryKind::Add,
        WordArithmeticKind::Sub => BinaryKind::Sub,
        WordArithmeticKind::Adc => BinaryKind::Adc,
        WordArithmeticKind::Sbb => BinaryKind::Sbb,
    };
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
        } => I::Binary(kind, location(destination), value(source), 4),
        Operation::ArithmeticWord {
            kind,
            destination,
            source,
        } => I::Binary(binary_word(kind), reg(destination), word(source), 2),
        Operation::ArithmeticByte {
            kind,
            destination,
            source,
        } => I::Binary(
            match kind {
                ByteArithmeticKind::Add => BinaryKind::Add,
                ByteArithmeticKind::Adc => BinaryKind::Adc,
                ByteArithmeticKind::Sub => BinaryKind::Sub,
                ByteArithmeticKind::Sbb => BinaryKind::Sbb,
            },
            byte(destination),
            byte_value(source),
            1,
        ),
        Operation::LogicalByte {
            kind,
            destination,
            source,
        } => I::Binary(
            match kind {
                ByteLogicalKind::And => BinaryKind::And,
                ByteLogicalKind::Or => BinaryKind::Or,
                ByteLogicalKind::Xor => BinaryKind::Xor,
            },
            byte(destination),
            byte_value(source),
            1,
        ),
        Operation::LogicalWord {
            kind,
            destination,
            source,
        } => I::Binary(
            match kind {
                WordLogicalKind::And => BinaryKind::And,
                WordLogicalKind::Or => BinaryKind::Or,
                WordLogicalKind::Xor => BinaryKind::Xor,
            },
            reg(destination),
            word(source),
            2,
        ),
        Operation::CompareByte { left, right } => {
            I::Binary(BinaryKind::Cmp, byte(left), byte_value(right), 1)
        }
        Operation::TestByte { left, right } => {
            I::Binary(BinaryKind::Test, byte(left), byte_value(right), 1)
        }
        Operation::CompareWord { left, right } => {
            I::Binary(BinaryKind::Cmp, reg(left), word(right), 2)
        }
        Operation::TestWord { left, right } => {
            I::Binary(BinaryKind::Test, reg(left), word(right), 2)
        }
        Operation::ReadArithmeticWord {
            kind,
            destination,
            address,
        } => I::Binary(
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
        } => I::Binary(
            match kind {
                ByteReadArithmeticKind::Add => BinaryKind::Add,
                ByteReadArithmeticKind::Adc => BinaryKind::Adc,
                ByteReadArithmeticKind::Sub => BinaryKind::Sub,
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
        } => I::Binary(
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
        } => I::Binary(
            match kind {
                MemoryByteArithmeticKind::Add => BinaryKind::Add,
                MemoryByteArithmeticKind::Adc => BinaryKind::Adc,
                MemoryByteArithmeticKind::Sub => BinaryKind::Sub,
                MemoryByteArithmeticKind::Sbb => BinaryKind::Sbb,
            },
            Operand::Memory(address),
            byte_value(source),
            1,
        ),
        Operation::Unary { kind, destination } => I::Unary(kind, location(destination), 4),
        Operation::UnaryByte { kind, destination } => I::Unary(kind, byte(destination), 1),
        Operation::UnaryWord { kind, destination } => I::Unary(kind, reg(destination), 2),
        Operation::MemoryUnaryByte { kind, address } => I::Unary(kind, Operand::Memory(address), 1),
        Operation::MemoryUnaryWord { kind, address } => I::Unary(kind, Operand::Memory(address), 2),
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
        Operation::ExchangeByte { left, right } => I::Exchange(byte(left), byte(right), 1),
        Operation::ExchangeWord { left, right } => I::Exchange(reg(left), reg(right), 2),
        Operation::Carry { kind } => I::Carry(kind),
        Operation::Direction { set } => I::Direction(set),
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
        Integer::Move(destination, source, width) => {
            out.operand(source, width, RESULT, false);
            out.store(destination, width, RESULT);
        }
        Integer::Binary(kind, destination, source, width) => {
            let writes = !matches!(kind, BinaryKind::Cmp | BinaryKind::Test);
            out.operand(destination, width, A, writes);
            out.operand(source, width, B, false);
            if matches!(kind, BinaryKind::Adc | BinaryKind::Sbb) {
                out.materialize(flags::CF);
                out.c()
                    .local_get(FLAGS)
                    .i32_const(1)
                    .i32_and()
                    .local_set(CARRY);
            } else {
                out.c().i32_const(0).local_set(CARRY);
            }
            out.c().local_get(A).local_get(B);
            match kind {
                BinaryKind::Add | BinaryKind::Adc => {
                    out.c().i32_add();
                }
                BinaryKind::Sub | BinaryKind::Sbb | BinaryKind::Cmp => {
                    out.c().i32_sub();
                }
                BinaryKind::And | BinaryKind::Test => {
                    out.c().i32_and();
                }
                BinaryKind::Or => {
                    out.c().i32_or();
                }
                BinaryKind::Xor => {
                    out.c().i32_xor();
                }
            }
            if matches!(kind, BinaryKind::Adc) {
                out.c().local_get(CARRY).i32_add();
            }
            if matches!(kind, BinaryKind::Sbb) {
                out.c().local_get(CARRY).i32_sub();
            }
            if width < 4 {
                out.c().i32_const(mask(width) as i32).i32_and();
            }
            out.c().local_set(RESULT);
            if writes {
                out.store(destination, width, RESULT);
            }
            out.recipe(
                match kind {
                    BinaryKind::Add => FlagKind::Add,
                    BinaryKind::Adc => FlagKind::Adc,
                    BinaryKind::Sub | BinaryKind::Cmp => FlagKind::Sub,
                    BinaryKind::Sbb => FlagKind::Sbb,
                    _ => FlagKind::Logical,
                },
                width,
            );
        }
        Integer::Unary(kind, destination, width) => {
            out.operand(destination, width, A, true);
            match kind {
                UnaryKind::Inc => {
                    out.c().local_get(A).i32_const(1).i32_add();
                }
                UnaryKind::Dec => {
                    out.c().local_get(A).i32_const(1).i32_sub();
                }
                UnaryKind::Not => {
                    out.c().local_get(A).i32_const(-1).i32_xor();
                }
                UnaryKind::Neg => {
                    out.c().i32_const(0).local_get(A).i32_sub();
                }
            }
            if width < 4 {
                out.c().i32_const(mask(width) as i32).i32_and();
            }
            out.c().local_set(RESULT);
            out.store(destination, width, RESULT);
            if !matches!(kind, UnaryKind::Not) {
                if out.record_flags && matches!(kind, UnaryKind::Inc | UnaryKind::Dec) {
                    out.materialize(flags::CF);
                }
                out.c().i32_const(0).local_set(CARRY);
                out.recipe(
                    match kind {
                        UnaryKind::Inc => FlagKind::Inc,
                        UnaryKind::Dec => FlagKind::Dec,
                        _ => FlagKind::Neg,
                    },
                    width,
                );
            }
        }
        Integer::Lea(destination, address, width) => {
            out.effective_address(address, false);
            out.c().local_get(ADDRESS).local_set(RESULT);
            out.store(reg(destination), width, RESULT);
        }
        Integer::Set(condition, destination) => {
            out.condition(condition);
            out.c().local_set(RESULT);
            out.store(destination, 1, RESULT);
        }
        Integer::ConditionalMove(condition, destination, source, width) => {
            out.operand(source, width, RESULT, false);
            out.operand(destination, width, A, false);
            out.condition(condition);
            out.c().local_set(B);
            out.c()
                .local_get(RESULT)
                .local_get(A)
                .local_get(B)
                .select()
                .local_set(RESULT);
            out.store(destination, width, RESULT);
        }
        Integer::Exchange(left, right, width) => {
            out.operand(left, width, A, false);
            out.operand(right, width, B, false);
            out.store(left, width, B);
            out.store(right, width, A);
        }
        Integer::Carry(kind) => {
            out.materialize(flags::ARITHMETIC);
            out.c().i32_const(0).local_set(KIND);
            out.c().local_get(FLAGS);
            match kind {
                CarryKind::Clear => {
                    out.c().i32_const(-2).i32_and();
                }
                CarryKind::Set => {
                    out.c().i32_const(1).i32_or();
                }
                CarryKind::Complement => {
                    out.c().i32_const(1).i32_xor();
                }
            }
            out.c().local_set(FLAGS);
        }
        Integer::Direction(decrement) => {
            out.c()
                .local_get(FLAGS)
                .i32_const(!0x400)
                .i32_and()
                .i32_const(if decrement { 0x400 } else { 0 })
                .i32_or()
                .local_set(FLAGS);
        }
    }
    true
}

fn mask(width: u32) -> u32 {
    u32::MAX >> (32 - width * 8)
}

impl Output<'_> {
    fn operand(&mut self, source: Operand, width: u32, destination: u32, modify: bool) {
        match source {
            Operand::Register(register, shift) => {
                self.c().local_get(register);
                if shift != 0 {
                    self.c().i32_const(shift as i32).i32_shr_u();
                }
                if width != 4 {
                    self.c().i32_const(mask(width) as i32).i32_and();
                }
                self.c().local_set(destination);
            }
            Operand::Immediate(value) => {
                self.c()
                    .i32_const((value & mask(width)) as i32)
                    .local_set(destination);
            }
            Operand::Memory(address) => {
                self.address(address);
                self.read(width | if modify { 0x100 } else { 0 }, destination);
            }
        }
    }

    fn store(&mut self, target: Operand, width: u32, source: u32) {
        match target {
            Operand::Register(register, shift) => {
                if width == 4 {
                    self.c().local_get(source).local_set(register);
                } else {
                    self.c()
                        .local_get(register)
                        .i32_const(!(mask(width) << shift) as i32)
                        .i32_and()
                        .local_get(source)
                        .i32_const(mask(width) as i32)
                        .i32_and();
                    if shift != 0 {
                        self.c().i32_const(shift as i32).i32_shl();
                    }
                    self.c().i32_or().local_set(register);
                }
            }
            Operand::Memory(address) => {
                self.address(address);
                self.write(width, source);
            }
            Operand::Immediate(_) => unreachable!("immediate destination"),
        }
    }

    fn recipe(&mut self, kind: FlagKind, width: u32) {
        if !self.record_flags {
            return;
        }
        self.c()
            .i32_const(kind as i32)
            .local_set(KIND)
            .i32_const((width * 8) as i32)
            .local_set(WIDTH);
        for (source, target) in [(A, LEFT), (B, RIGHT), (RESULT, VALUE), (CARRY, CARRY_IN)] {
            self.c().local_get(source).local_set(target);
        }
    }

    pub(super) fn load_location(&mut self, source: Location32, width: u32, destination: u32) {
        self.operand(location(source), width, destination, false);
    }
    pub(super) fn load_value(&mut self, source: Value32, width: u32, destination: u32) {
        self.operand(value(source), width, destination, false);
    }
    pub(super) fn address(&mut self, address: EffectiveAddress) {
        self.effective_address(address, true);
    }

    fn effective_address(&mut self, address: EffectiveAddress, segment: bool) {
        self.c().i32_const(address.displacement as i32);
        if let Some(base) = address.base {
            self.c().local_get(base.index() as u32 + 2).i32_add();
        }
        if let Some(index) = address.index {
            self.c()
                .local_get(index.index() as u32 + 2)
                .i32_const(address.scale as i32)
                .i32_mul()
                .i32_add();
        }
        if address.address_size == AddressSize::Bits16 {
            self.c().i32_const(0xffff).i32_and();
        }
        if segment {
            self.c().local_tee(super::LOGICAL_ADDRESS);
            let offset = match address.resolved_segment() {
                SegmentRegister::Fs => Some(104),
                SegmentRegister::Gs => Some(108),
                _ => None,
            };
            if let Some(offset) = offset {
                self.c()
                    .local_get(super::CTX)
                    .i32_load(super::mem(offset, 2))
                    .i32_add();
            }
        }
        self.c().local_set(ADDRESS);
    }
}
