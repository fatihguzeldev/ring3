use super::*;

#[derive(Clone, Copy)]
enum Place {
    Byte(ByteRegister),
    Word(Register32),
    Memory(EffectiveAddress),
}

enum Input {
    Byte(ByteValue),
    Word(WordValue),
    Memory(EffectiveAddress),
}

macro_rules! arithmetic {
    ($kind:expr, $class:ident) => {
        match $kind {
            $class::Add => BinaryKind::Add,
            $class::Adc => BinaryKind::Adc,
            $class::Sub => BinaryKind::Sub,
            $class::Sbb => BinaryKind::Sbb,
        }
    };
}

macro_rules! logical {
    ($kind:expr, $class:ident) => {
        match $kind {
            $class::And => BinaryKind::And,
            $class::Or => BinaryKind::Or,
            $class::Xor => BinaryKind::Xor,
        }
    };
}

pub(super) fn supports(operation: &Operation) -> bool {
    matches!(
        operation,
        Operation::ArithmeticWord { .. }
            | Operation::ReadArithmeticWord { .. }
            | Operation::MemoryArithmeticWord { .. }
            | Operation::LogicalWord { .. }
            | Operation::ReadLogicalWord { .. }
            | Operation::MemoryLogicalWord { .. }
            | Operation::CompareWord { .. }
            | Operation::TestWord { .. }
            | Operation::MemoryCompareWord { .. }
            | Operation::ReadCompareWord { .. }
            | Operation::MemoryTestWord { .. }
            | Operation::ArithmeticByte { .. }
            | Operation::ReadArithmeticByte { .. }
            | Operation::MemoryArithmeticByte { .. }
            | Operation::LogicalByte { .. }
            | Operation::ReadLogicalByte { .. }
            | Operation::MemoryLogicalByte { .. }
            | Operation::CompareByte { .. }
            | Operation::ReadCompareByte { .. }
            | Operation::TestByte { .. }
            | Operation::MemoryPredicateByte { .. }
            | Operation::UnaryByte { .. }
            | Operation::MemoryUnaryByte { .. }
            | Operation::UnaryWord { .. }
            | Operation::MemoryUnaryWord { .. }
    )
}

pub(super) fn emit(out: &mut Output, operation: Operation) -> bool {
    let (kind, width, destination, source) = match operation {
        Operation::ArithmeticWord {
            kind,
            destination,
            source,
        } => (
            arithmetic!(kind, WordArithmeticKind),
            2,
            Place::Word(destination),
            Input::Word(source),
        ),
        Operation::ReadArithmeticWord {
            kind,
            destination,
            address,
        } => (
            arithmetic!(kind, WordReadArithmeticKind),
            2,
            Place::Word(destination),
            Input::Memory(address),
        ),
        Operation::MemoryArithmeticWord {
            kind,
            address,
            source,
        } => (
            arithmetic!(kind, WordMemoryArithmeticKind),
            2,
            Place::Memory(address),
            Input::Word(source),
        ),
        Operation::LogicalWord {
            kind,
            destination,
            source,
        } => (
            logical!(kind, WordLogicalKind),
            2,
            Place::Word(destination),
            Input::Word(source),
        ),
        Operation::ReadLogicalWord {
            kind,
            destination,
            address,
        } => (
            logical!(kind, WordLogicalKind),
            2,
            Place::Word(destination),
            Input::Memory(address),
        ),
        Operation::MemoryLogicalWord {
            kind,
            address,
            source,
        } => (
            logical!(kind, WordLogicalKind),
            2,
            Place::Memory(address),
            Input::Word(source),
        ),
        Operation::CompareWord { left, right } => {
            (BinaryKind::Cmp, 2, Place::Word(left), Input::Word(right))
        }
        Operation::TestWord { left, right } => {
            (BinaryKind::Test, 2, Place::Word(left), Input::Word(right))
        }
        Operation::MemoryCompareWord { address, right } => (
            BinaryKind::Cmp,
            2,
            Place::Memory(address),
            Input::Word(right),
        ),
        Operation::ReadCompareWord { left, address } => (
            BinaryKind::Cmp,
            2,
            Place::Word(left),
            Input::Memory(address),
        ),
        Operation::MemoryTestWord { address, right } => (
            BinaryKind::Test,
            2,
            Place::Memory(address),
            Input::Word(right),
        ),
        Operation::ArithmeticByte {
            kind,
            destination,
            source,
        } => (
            arithmetic!(kind, ByteArithmeticKind),
            1,
            Place::Byte(destination),
            Input::Byte(source),
        ),
        Operation::ReadArithmeticByte {
            kind,
            destination,
            address,
        } => (
            arithmetic!(kind, ByteReadArithmeticKind),
            1,
            Place::Byte(destination),
            Input::Memory(address),
        ),
        Operation::MemoryArithmeticByte {
            kind,
            address,
            source,
        } => (
            arithmetic!(kind, MemoryByteArithmeticKind),
            1,
            Place::Memory(address),
            Input::Byte(source),
        ),
        Operation::LogicalByte {
            kind,
            destination,
            source,
        } => (
            logical!(kind, ByteLogicalKind),
            1,
            Place::Byte(destination),
            Input::Byte(source),
        ),
        Operation::ReadLogicalByte {
            kind,
            destination,
            address,
        } => (
            logical!(kind, ByteLogicalKind),
            1,
            Place::Byte(destination),
            Input::Memory(address),
        ),
        Operation::MemoryLogicalByte {
            kind,
            address,
            source,
        } => (
            logical!(kind, ByteLogicalKind),
            1,
            Place::Memory(address),
            Input::Byte(source),
        ),
        Operation::CompareByte { left, right } => {
            (BinaryKind::Cmp, 1, Place::Byte(left), Input::Byte(right))
        }
        Operation::ReadCompareByte { left, address } => (
            BinaryKind::Cmp,
            1,
            Place::Byte(left),
            Input::Memory(address),
        ),
        Operation::TestByte { left, right } => {
            (BinaryKind::Test, 1, Place::Byte(left), Input::Byte(right))
        }
        Operation::MemoryPredicateByte {
            kind,
            address,
            right,
        } => (
            if kind == BytePredicateKind::Cmp {
                BinaryKind::Cmp
            } else {
                BinaryKind::Test
            },
            1,
            Place::Memory(address),
            Input::Byte(right),
        ),
        Operation::UnaryByte { kind, destination } => {
            out.narrow_unary(kind, 1, Place::Byte(destination));
            return true;
        }
        Operation::MemoryUnaryByte { kind, address } => {
            out.narrow_unary(kind, 1, Place::Memory(address));
            return true;
        }
        Operation::UnaryWord { kind, destination } => {
            out.narrow_unary(kind, 2, Place::Word(destination));
            return true;
        }
        Operation::MemoryUnaryWord { kind, address } => {
            out.narrow_unary(kind, 2, Place::Memory(address));
            return true;
        }
        _ => return false,
    };
    let write = !matches!(kind, BinaryKind::Cmp | BinaryKind::Test);
    out.narrow_read(destination, width, write);
    match source {
        Input::Byte(value) => out.byte(value, B),
        Input::Word(value) => out.word(value, B),
        Input::Memory(address) => {
            out.address(address);
            out.read(width, B);
        }
    }
    out.binary(kind);
    out.narrow_result(width);
    if write {
        out.narrow_store(destination, width);
    }
    out.flags(kind, width * 8, false);
    true
}

impl Output {
    fn narrow_read(&mut self, destination: Place, width: u32, modify: bool) {
        match destination {
            Place::Byte(register) => self.byte(ByteValue::Register(register), A),
            Place::Word(register) => self.word(WordValue::Register(register), A),
            Place::Memory(address) => {
                self.address(address);
                self.c().local_get(ADDRESS).local_set(DESTINATION);
                self.read(width | if modify { 0x100 } else { 0 }, A);
            }
        }
    }
    fn narrow_result(&mut self, width: u32) {
        self.c()
            .local_get(R)
            .i32_const(if width == 1 { 255 } else { 0xffff })
            .i32_and()
            .local_set(R);
    }
    fn narrow_store(&mut self, destination: Place, width: u32) {
        match destination {
            Place::Byte(register) => self.store_byte(register, R),
            Place::Word(register) => self.store_word(register, R),
            Place::Memory(_) => {
                self.c().local_get(DESTINATION).local_set(ADDRESS);
                self.write(width, R);
            }
        }
    }
    fn narrow_unary(&mut self, kind: UnaryKind, width: u32, destination: Place) {
        self.narrow_read(destination, width, true);
        let binary = match kind {
            UnaryKind::Not => {
                self.c().local_get(A).i32_const(-1).i32_xor().local_set(R);
                self.narrow_result(width);
                self.narrow_store(destination, width);
                return;
            }
            UnaryKind::Inc | UnaryKind::Dec => {
                self.c().i32_const(1).local_set(B);
                if kind == UnaryKind::Inc {
                    BinaryKind::Add
                } else {
                    BinaryKind::Sub
                }
            }
            UnaryKind::Neg => {
                self.c().local_get(A).local_set(B).i32_const(0).local_set(A);
                BinaryKind::Sub
            }
        };
        self.binary(binary);
        self.narrow_result(width);
        self.narrow_store(destination, width);
        self.flags(
            binary,
            width * 8,
            matches!(kind, UnaryKind::Inc | UnaryKind::Dec),
        );
    }
}
