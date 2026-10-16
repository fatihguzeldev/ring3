use super::*;

pub(super) fn emit(
    code: &mut InstructionSink<'_>,
    instruction: &DecodedOperation,
    imports: Imports,
    exit_depth: u32,
) -> bool {
    let mut store = false;
    match *instruction.operation() {
        Operation::AtomicMemory {
            kind,
            width,
            address,
            source,
        } => {
            atomic::operation(code, kind, width, address, source, imports, exit_depth);
            store = true;
        }
        Operation::CompareExchange8B { address } => {
            atomic::compare8(code, address, imports, exit_depth);
            store = true;
        }
        Operation::ExchangeWord { left, right } => {
            code.local_get(register(left))
                .i32_const(!0xffff)
                .i32_and()
                .local_get(register(right))
                .i32_const(0xffff)
                .i32_and()
                .i32_or()
                .local_get(register(right))
                .i32_const(!0xffff)
                .i32_and()
                .local_get(register(left))
                .i32_const(0xffff)
                .i32_and()
                .i32_or()
                .local_set(register(right))
                .local_set(register(left));
        }
        Operation::Exchange { left, right } => {
            code.local_get(register(left))
                .local_get(register(right))
                .local_set(register(left))
                .local_set(register(right));
        }
        Operation::MemoryExchange { address, source } => {
            memory::exchange_result(code, address, source, imports, exit_depth);
            code.local_get(RESULT).local_set(register(source));
            store = true;
        }
        Operation::MemoryExchangeByte { address, source } => {
            byte_value(code, ByteValue::Register(source));
            code.local_set(RESULT);
            memory::exchange_byte_result(code, address, imports, exit_depth);
            code.local_get(RESULT);
            insert_byte(code, source);
            store = true;
        }
        Operation::MemoryExchangeWord { address, source } => {
            word_value(code, WordValue::Register(source));
            code.local_set(RESULT);
            memory::exchange_word_result(code, address, imports, exit_depth);
            code.local_get(RESULT)
                .local_get(register(source))
                .i32_const(!0xffff)
                .i32_and()
                .i32_or()
                .local_set(register(source));
            store = true;
        }
        Operation::ExchangeAdd {
            destination,
            source,
        } => {
            code.local_get(register(destination))
                .local_set(LHS)
                .local_get(register(source))
                .local_set(RHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_add()
                .local_set(RESULT);
            arithmetic_flags(code, BinaryKind::Add, CarryFlag::Calculate, 31);
            code.local_get(LHS)
                .local_set(register(source))
                .local_get(RESULT)
                .local_set(register(destination));
        }
        Operation::ExchangeAddWord {
            destination,
            source,
        } => {
            word_value(code, WordValue::Register(destination));
            code.local_set(LHS);
            word_value(code, WordValue::Register(source));
            code.local_set(RHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_add()
                .i32_const(0xffff)
                .i32_and()
                .local_set(RESULT);
            arithmetic_flags(code, BinaryKind::Add, CarryFlag::Calculate, 15);
            code.local_get(LHS)
                .local_get(register(source))
                .i32_const(!0xffff)
                .i32_and()
                .i32_or()
                .local_set(register(source))
                .local_get(RESULT)
                .local_get(register(destination))
                .i32_const(!0xffff)
                .i32_and()
                .i32_or()
                .local_set(register(destination));
        }
        Operation::MemoryExchangeAddWord { address, source } => {
            memory::load_narrow_for_modify(code, address, SmallWidth::Word, imports, exit_depth);
            code.local_set(LHS);
            word_value(code, WordValue::Register(source));
            code.local_set(RHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_add()
                .i32_const(0xffff)
                .i32_and()
                .local_set(RESULT)
                .local_get(LHS)
                .local_get(RHS);
            memory::store_word_result(code, address, imports, exit_depth);
            code.local_set(RHS).local_set(LHS);
            arithmetic_flags(code, BinaryKind::Add, CarryFlag::Calculate, 15);
            code.local_get(LHS)
                .local_get(register(source))
                .i32_const(!0xffff)
                .i32_and()
                .i32_or()
                .local_set(register(source));
            store = true;
        }
        Operation::MemoryExchangeAdd { address, source } => {
            memory::load_modify_result(code, address, imports, exit_depth);
            code.local_get(RESULT)
                .local_set(LHS)
                .local_get(register(source))
                .local_set(RHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_add()
                .local_set(RESULT)
                .local_get(LHS)
                .local_get(RHS);
            memory::store_result(code, address, imports, exit_depth);
            code.local_set(RHS).local_set(LHS);
            arithmetic_flags(code, BinaryKind::Add, CarryFlag::Calculate, 31);
            code.local_get(LHS).local_set(register(source));
            store = true;
        }
        Operation::LockedMemoryExchangeAdd32 { address, source } => {
            memory::exchange_add_result(code, address, source, imports, exit_depth);
            code.local_get(RESULT)
                .local_set(LHS)
                .local_get(register(source))
                .local_set(RHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_add()
                .local_set(RESULT);
            arithmetic_flags(code, BinaryKind::Add, CarryFlag::Calculate, 31);
            code.local_get(LHS).local_set(register(source));
            store = true;
        }
        Operation::ExchangeAddByte {
            destination,
            source,
        } => {
            byte_value(code, ByteValue::Register(destination));
            code.local_set(LHS);
            byte_value(code, ByteValue::Register(source));
            code.local_set(RHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_add()
                .i32_const(0xff)
                .i32_and()
                .local_set(RESULT);
            arithmetic_flags(code, BinaryKind::Add, CarryFlag::Calculate, 7);
            code.local_get(LHS);
            insert_byte(code, source);
            code.local_get(RESULT);
            insert_byte(code, destination);
        }
        Operation::LockedMemoryExchangeAddByte { address, source } => {
            byte_value(code, ByteValue::Register(source));
            code.local_set(RESULT);
            memory::exchange_add_byte_result(code, address, imports, exit_depth);
            code.local_get(RESULT).local_set(LHS);
            byte_value(code, ByteValue::Register(source));
            code.local_set(RHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_add()
                .i32_const(0xff)
                .i32_and()
                .local_set(RESULT);
            arithmetic_flags(code, BinaryKind::Add, CarryFlag::Calculate, 7);
            code.local_get(LHS);
            insert_byte(code, source);
            store = true;
        }
        Operation::LockedMemoryExchangeAddWord { address, source } => {
            word_value(code, WordValue::Register(source));
            code.local_set(RESULT);
            memory::exchange_add_word_result(code, address, imports, exit_depth);
            code.local_get(RESULT).local_set(LHS);
            word_value(code, WordValue::Register(source));
            code.local_set(RHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_add()
                .i32_const(0xffff)
                .i32_and()
                .local_set(RESULT);
            arithmetic_flags(code, BinaryKind::Add, CarryFlag::Calculate, 15);
            code.local_get(LHS)
                .local_get(register(source))
                .i32_const(!0xffff)
                .i32_and()
                .i32_or()
                .local_set(register(source));
            store = true;
        }
        Operation::MemoryExchangeAddByte { address, source } => {
            memory::load_narrow_for_modify(code, address, SmallWidth::Byte, imports, exit_depth);
            code.local_set(LHS);
            byte_value(code, ByteValue::Register(source));
            code.local_set(RHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_add()
                .i32_const(0xff)
                .i32_and()
                .local_set(RESULT)
                .local_get(LHS)
                .local_get(RHS);
            memory::store_byte_result(code, address, imports, exit_depth);
            code.local_set(RHS).local_set(LHS);
            arithmetic_flags(code, BinaryKind::Add, CarryFlag::Calculate, 7);
            code.local_get(LHS);
            insert_byte(code, source);
            store = true;
        }
        Operation::CompareExchange {
            destination,
            source,
        } => {
            code.local_get(register(Register32::Eax))
                .local_set(LHS)
                .local_get(register(destination))
                .local_set(RHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_sub()
                .local_set(RESULT);
            arithmetic_flags(code, BinaryKind::Cmp, CarryFlag::Calculate, 31);
            code.local_get(RESULT)
                .i32_eqz()
                .if_(BlockType::Empty)
                .local_get(register(source))
                .local_set(register(destination))
                .else_()
                .local_get(RHS)
                .local_set(register(Register32::Eax))
                .end();
        }
        Operation::CompareExchangeWord {
            destination,
            source,
        } => {
            word_value(code, WordValue::Register(Register32::Eax));
            code.local_set(LHS);
            word_value(code, WordValue::Register(destination));
            code.local_set(RHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_sub()
                .i32_const(0xffff)
                .i32_and()
                .local_set(RESULT);
            arithmetic_flags(code, BinaryKind::Cmp, CarryFlag::Calculate, 15);
            code.local_get(RESULT).i32_eqz().if_(BlockType::Empty);
            word_value(code, WordValue::Register(source));
            code.local_get(register(destination))
                .i32_const(!0xffff)
                .i32_and()
                .i32_or()
                .local_set(register(destination))
                .else_()
                .local_get(RHS)
                .local_get(register(Register32::Eax))
                .i32_const(!0xffff)
                .i32_and()
                .i32_or()
                .local_set(register(Register32::Eax))
                .end();
        }
        Operation::LockedMemoryCompareExchangeWord { address, source } => {
            word_value(code, WordValue::Register(Register32::Eax));
            word_value(code, WordValue::Register(source));
            code.local_set(RESULT);
            memory::compare_exchange_word_result(code, address, imports, exit_depth);
            code.local_set(LHS)
                .local_get(RESULT)
                .local_set(RHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_sub()
                .i32_const(0xffff)
                .i32_and()
                .local_set(RESULT);
            arithmetic_flags(code, BinaryKind::Cmp, CarryFlag::Calculate, 15);
            code.local_get(RESULT)
                .if_(BlockType::Empty)
                .local_get(RHS)
                .local_get(register(Register32::Eax))
                .i32_const(!0xffff)
                .i32_and()
                .i32_or()
                .local_set(register(Register32::Eax))
                .end();
            store = true;
        }
        Operation::MemoryCompareExchangeWord { address, source } => {
            memory::load_narrow_for_modify(code, address, SmallWidth::Word, imports, exit_depth);
            code.local_set(RHS);
            word_value(code, WordValue::Register(Register32::Eax));
            code.local_set(LHS);
            word_value(code, WordValue::Register(source));
            code.local_get(RHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_eq()
                .select()
                .local_set(RESULT)
                .local_get(LHS)
                .local_get(RHS);
            memory::store_word_result(code, address, imports, exit_depth);
            code.local_set(RHS)
                .local_set(LHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_sub()
                .i32_const(0xffff)
                .i32_and()
                .local_set(RESULT);
            arithmetic_flags(code, BinaryKind::Cmp, CarryFlag::Calculate, 15);
            code.local_get(RESULT)
                .if_(BlockType::Empty)
                .local_get(RHS)
                .local_get(register(Register32::Eax))
                .i32_const(!0xffff)
                .i32_and()
                .i32_or()
                .local_set(register(Register32::Eax))
                .end();
            store = true;
        }
        Operation::LockedMemoryCompareExchange32 { address, source } => {
            code.local_get(register(Register32::Eax));
            memory::compare_exchange_result(code, address, source, imports, exit_depth);
            code.local_set(LHS)
                .local_get(RESULT)
                .local_set(RHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_sub()
                .local_set(RESULT);
            arithmetic_flags(code, BinaryKind::Cmp, CarryFlag::Calculate, 31);
            code.local_get(RESULT)
                .if_(BlockType::Empty)
                .local_get(RHS)
                .local_set(register(Register32::Eax))
                .end();
            store = true;
        }
        Operation::MemoryCompareExchange { address, source } => {
            memory::load_modify_result(code, address, imports, exit_depth);
            code.local_get(register(Register32::Eax))
                .local_set(LHS)
                .local_get(RESULT)
                .local_set(RHS)
                .local_get(register(source))
                .local_get(RHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_eq()
                .select()
                .local_set(RESULT)
                .local_get(LHS)
                .local_get(RHS);
            memory::store_result(code, address, imports, exit_depth);
            code.local_set(RHS)
                .local_set(LHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_sub()
                .local_set(RESULT);
            arithmetic_flags(code, BinaryKind::Cmp, CarryFlag::Calculate, 31);
            code.local_get(RESULT)
                .if_(BlockType::Empty)
                .local_get(RHS)
                .local_set(register(Register32::Eax))
                .end();
            store = true;
        }
        Operation::CompareExchangeByte {
            destination,
            source,
        } => {
            byte_value(code, ByteValue::Register(ByteRegister::Al));
            code.local_set(LHS);
            byte_value(code, ByteValue::Register(destination));
            code.local_set(RHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_sub()
                .i32_const(0xff)
                .i32_and()
                .local_set(RESULT);
            arithmetic_flags(code, BinaryKind::Cmp, CarryFlag::Calculate, 7);
            code.local_get(RESULT).i32_eqz().if_(BlockType::Empty);
            byte_value(code, ByteValue::Register(source));
            insert_byte(code, destination);
            code.else_().local_get(RHS);
            insert_byte(code, ByteRegister::Al);
            code.end();
        }
        Operation::LockedMemoryCompareExchangeByte { address, source } => {
            byte_value(code, ByteValue::Register(ByteRegister::Al));
            byte_value(code, ByteValue::Register(source));
            code.local_set(RESULT);
            memory::compare_exchange_byte_result(code, address, imports, exit_depth);
            code.local_set(LHS)
                .local_get(RESULT)
                .local_set(RHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_sub()
                .i32_const(0xff)
                .i32_and()
                .local_set(RESULT);
            arithmetic_flags(code, BinaryKind::Cmp, CarryFlag::Calculate, 7);
            code.local_get(RESULT).if_(BlockType::Empty).local_get(RHS);
            insert_byte(code, ByteRegister::Al);
            code.end();
            store = true;
        }
        Operation::MemoryCompareExchangeByte { address, source } => {
            memory::load_narrow_for_modify(code, address, SmallWidth::Byte, imports, exit_depth);
            code.local_set(RHS);
            byte_value(code, ByteValue::Register(ByteRegister::Al));
            code.local_set(LHS);
            byte_value(code, ByteValue::Register(source));
            code.local_get(RHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_eq()
                .select()
                .local_set(RESULT)
                .local_get(LHS)
                .local_get(RHS);
            memory::store_byte_result(code, address, imports, exit_depth);
            code.local_set(RHS)
                .local_set(LHS)
                .local_get(LHS)
                .local_get(RHS)
                .i32_sub()
                .i32_const(0xff)
                .i32_and()
                .local_set(RESULT);
            arithmetic_flags(code, BinaryKind::Cmp, CarryFlag::Calculate, 7);
            code.local_get(RESULT).if_(BlockType::Empty).local_get(RHS);
            insert_byte(code, ByteRegister::Al);
            code.end();
            store = true;
        }
        Operation::ExchangeByte { left, right } => {
            byte_value(code, ByteValue::Register(left));
            byte_value(code, ByteValue::Register(right));
            insert_byte(code, left);
            insert_byte(code, right);
        }
        _ => unreachable!("instruction routed to exchange emitter"),
    }
    store
}
