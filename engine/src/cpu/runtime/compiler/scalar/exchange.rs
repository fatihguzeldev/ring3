use super::*;

pub(super) fn supports(operation: &Operation) -> bool {
    matches!(
        operation,
        Operation::ExchangeByte { .. }
            | Operation::ExchangeWord { .. }
            | Operation::MemoryExchange { .. }
            | Operation::MemoryExchangeByte { .. }
            | Operation::MemoryExchangeWord { .. }
    )
}

#[derive(Clone, Copy)]
enum Register {
    Byte(ByteRegister),
    Word(Register32),
    Dword(Register32),
}

pub(super) fn emit(out: &mut Output, operation: Operation) -> bool {
    let (address, register, width) = match operation {
        Operation::ExchangeByte { left, right } => {
            out.byte(ByteValue::Register(left), A);
            out.byte(ByteValue::Register(right), B);
            out.store_byte(left, B);
            out.store_byte(right, A);
            return true;
        }
        Operation::ExchangeWord { left, right } => {
            out.word(WordValue::Register(left), A);
            out.word(WordValue::Register(right), B);
            out.store_word(left, B);
            out.store_word(right, A);
            return true;
        }
        Operation::MemoryExchange { address, source } => (address, Register::Dword(source), 4),
        Operation::MemoryExchangeWord { address, source } => (address, Register::Word(source), 2),
        Operation::MemoryExchangeByte { address, source } => (address, Register::Byte(source), 1),
        _ => return false,
    };
    match register {
        Register::Byte(register) => out.byte(ByteValue::Register(register), B),
        Register::Word(register) => out.word(WordValue::Register(register), B),
        Register::Dword(register) => {
            out.c().local_get(reg(register)).local_set(B);
        }
    }
    out.address(address);
    out.read(width | 0x100, A);
    out.write(width, B);
    match register {
        Register::Byte(register) => out.store_byte(register, A),
        Register::Word(register) => out.store_word(register, A),
        Register::Dword(register) => {
            out.c().local_get(A).local_set(reg(register));
        }
    }
    true
}
