use super::{
    BinaryKind, ByteLogicalKind, ByteRegister, ByteValue, CarryFlag, EffectiveAddress, FLAGS,
    InstructionSink, LHS, MemoryByteArithmeticKind, RESULT, RHS, Register32, SmallWidth, UnaryKind,
    Value32, WordValue, arithmetic_flags, byte_value, insert_byte, logical_flags, memory, register,
    value, word_value,
};

pub(super) fn unary_memory_byte(
    code: &mut InstructionSink<'_>,
    kind: UnaryKind,
    address: EffectiveAddress,
    imports: memory::Imports,
    exit_depth: u32,
) {
    memory::load_narrow_for_modify(code, address, SmallWidth::Byte, imports, exit_depth);
    code.local_set(RESULT);
    match kind {
        UnaryKind::Inc => {
            code.local_get(RESULT).i32_const(1).i32_add();
        }
        UnaryKind::Dec => {
            code.local_get(RESULT).i32_const(1).i32_sub();
        }
        UnaryKind::Neg => {
            code.i32_const(0).local_get(RESULT).i32_sub();
        }
        UnaryKind::Not => {
            code.local_get(RESULT).i32_const(0xff).i32_xor();
        }
    }
    code.i32_const(0xff).i32_and().local_set(RESULT);
    memory::store_byte_result(code, address, imports, exit_depth);
    // store validation uses LHS/RHS scratch; recover operands only after success.
    let (binary, carry) = match kind {
        UnaryKind::Inc => {
            code.local_get(RESULT)
                .i32_const(1)
                .i32_sub()
                .i32_const(0xff)
                .i32_and()
                .local_set(LHS)
                .i32_const(1)
                .local_set(RHS);
            (BinaryKind::Add, CarryFlag::Preserve)
        }
        UnaryKind::Dec => {
            code.local_get(RESULT)
                .i32_const(1)
                .i32_add()
                .i32_const(0xff)
                .i32_and()
                .local_set(LHS)
                .i32_const(1)
                .local_set(RHS);
            (BinaryKind::Sub, CarryFlag::Preserve)
        }
        UnaryKind::Neg => {
            code.i32_const(0)
                .local_set(LHS)
                .i32_const(0)
                .local_get(RESULT)
                .i32_sub()
                .i32_const(0xff)
                .i32_and()
                .local_set(RHS);
            (BinaryKind::Sub, CarryFlag::Calculate)
        }
        UnaryKind::Not => return,
    };
    arithmetic_flags(code, binary, carry, 7);
}

pub(super) fn unary_memory_word(
    code: &mut InstructionSink<'_>,
    kind: UnaryKind,
    address: EffectiveAddress,
    imports: memory::Imports,
    exit_depth: u32,
) {
    memory::load_narrow_for_modify(code, address, SmallWidth::Word, imports, exit_depth);
    code.local_set(RESULT);
    match kind {
        UnaryKind::Inc => {
            code.local_get(RESULT).i32_const(1).i32_add();
        }
        UnaryKind::Dec => {
            code.local_get(RESULT).i32_const(1).i32_sub();
        }
        UnaryKind::Neg => {
            code.i32_const(0).local_get(RESULT).i32_sub();
        }
        UnaryKind::Not => {
            code.local_get(RESULT).i32_const(0xffff).i32_xor();
        }
    }
    code.i32_const(0xffff).i32_and().local_set(RESULT);
    memory::store_word_result(code, address, imports, exit_depth);
    // store validation uses operand scratch; recover the original word after success.
    let (binary, carry) = match kind {
        UnaryKind::Inc | UnaryKind::Dec => {
            code.local_get(RESULT).i32_const(1);
            let binary = if kind == UnaryKind::Inc {
                code.i32_sub();
                BinaryKind::Add
            } else {
                code.i32_add();
                BinaryKind::Sub
            };
            code.i32_const(0xffff)
                .i32_and()
                .local_set(LHS)
                .i32_const(1)
                .local_set(RHS);
            (binary, CarryFlag::Preserve)
        }
        UnaryKind::Neg => {
            code.i32_const(0)
                .local_set(LHS)
                .i32_const(0)
                .local_get(RESULT)
                .i32_sub()
                .i32_const(0xffff)
                .i32_and()
                .local_set(RHS);
            (BinaryKind::Sub, CarryFlag::Calculate)
        }
        UnaryKind::Not => return,
    };
    arithmetic_flags(code, binary, carry, 15);
}

pub(super) fn logical_memory_byte(
    code: &mut InstructionSink<'_>,
    kind: ByteLogicalKind,
    address: EffectiveAddress,
    source: ByteValue,
    imports: memory::Imports,
    exit_depth: u32,
) {
    memory::load_narrow_for_modify(code, address, SmallWidth::Byte, imports, exit_depth);
    byte_value(code, source);
    match kind {
        ByteLogicalKind::And => {
            code.i32_and();
        }
        ByteLogicalKind::Or => {
            code.i32_or();
        }
        ByteLogicalKind::Xor => {
            code.i32_xor();
        }
    }
    code.local_set(RESULT);
    memory::store_byte_result(code, address, imports, exit_depth);
    logical_flags(code, 7);
}

pub(super) fn arithmetic_memory_byte(
    code: &mut InstructionSink<'_>,
    kind: MemoryByteArithmeticKind,
    address: EffectiveAddress,
    source: ByteValue,
    imports: memory::Imports,
    exit_depth: u32,
) {
    memory::load_narrow_for_modify(code, address, SmallWidth::Byte, imports, exit_depth);
    byte_value(code, source);
    let binary = match kind {
        MemoryByteArithmeticKind::Add => {
            code.i32_add();
            BinaryKind::Add
        }
        MemoryByteArithmeticKind::Adc => {
            code.i32_add();
            BinaryKind::Adc
        }
        MemoryByteArithmeticKind::Sub => {
            code.i32_sub();
            BinaryKind::Sub
        }
        MemoryByteArithmeticKind::Sbb => {
            code.i32_sub();
            BinaryKind::Sbb
        }
    };
    if matches!(binary, BinaryKind::Adc | BinaryKind::Sbb) {
        code.local_get(FLAGS).i32_const(1).i32_and();
        if binary == BinaryKind::Adc {
            code.i32_add();
        } else {
            code.i32_sub();
        }
    }
    code.i32_const(0xff).i32_and().local_set(RESULT);
    memory::store_byte_result(code, address, imports, exit_depth);
    // store validation uses operand scratch; recover the original byte after success.
    byte_value(code, source);
    code.local_set(RHS).local_get(RESULT).local_get(RHS);
    if matches!(
        kind,
        MemoryByteArithmeticKind::Add | MemoryByteArithmeticKind::Adc
    ) {
        code.i32_sub();
    } else {
        code.i32_add();
    }
    if matches!(binary, BinaryKind::Adc | BinaryKind::Sbb) {
        code.local_get(FLAGS).i32_const(1).i32_and();
        if binary == BinaryKind::Adc {
            code.i32_sub();
        } else {
            code.i32_add();
        }
    }
    code.i32_const(0xff).i32_and().local_set(LHS);
    arithmetic_flags(code, binary, CarryFlag::Calculate, 7);
}

pub(super) fn binary_memory_store(
    code: &mut InstructionSink<'_>,
    kind: BinaryKind,
    address: EffectiveAddress,
    source: Value32,
    imports: memory::Imports,
    exit_depth: u32,
) {
    memory::store_result(code, address, imports, exit_depth);
    if matches!(
        kind,
        BinaryKind::Add | BinaryKind::Adc | BinaryKind::Sub | BinaryKind::Sbb
    ) {
        // store validation uses operand scratch; recover the old destination from the unchanged source.
        value(code, source);
        code.local_set(RHS).local_get(RESULT).local_get(RHS);
        if matches!(kind, BinaryKind::Add | BinaryKind::Adc) {
            code.i32_sub();
        } else {
            code.i32_add();
        }
        if matches!(kind, BinaryKind::Adc | BinaryKind::Sbb) {
            code.local_get(FLAGS).i32_const(1).i32_and();
            if kind == BinaryKind::Adc {
                code.i32_sub();
            } else {
                code.i32_add();
            }
        }
        code.local_set(LHS);
    }
}

pub(super) fn unary_memory(
    code: &mut InstructionSink<'_>,
    kind: UnaryKind,
    address: EffectiveAddress,
    imports: memory::Imports,
    exit_depth: u32,
) {
    memory::load_modify_result(code, address, imports, exit_depth);
    match kind {
        UnaryKind::Inc => {
            code.local_get(RESULT).i32_const(1).i32_add();
        }
        UnaryKind::Dec => {
            code.local_get(RESULT).i32_const(1).i32_sub();
        }
        UnaryKind::Not => {
            code.local_get(RESULT).i32_const(-1).i32_xor();
        }
        UnaryKind::Neg => {
            code.i32_const(0).local_get(RESULT).i32_sub();
        }
    }
    code.local_set(RESULT);
    memory::store_result(code, address, imports, exit_depth);

    // store validation uses operand scratch; recover reversible operands after it succeeds.
    let (binary, carry) = match kind {
        UnaryKind::Not => return,
        UnaryKind::Inc | UnaryKind::Dec => {
            code.local_get(RESULT).i32_const(1);
            let binary = if kind == UnaryKind::Inc {
                code.i32_sub();
                BinaryKind::Add
            } else {
                code.i32_add();
                BinaryKind::Sub
            };
            code.local_set(LHS).i32_const(1).local_set(RHS);
            (binary, CarryFlag::Preserve)
        }
        UnaryKind::Neg => {
            code.i32_const(0)
                .local_set(LHS)
                .i32_const(0)
                .local_get(RESULT)
                .i32_sub()
                .local_set(RHS);
            (BinaryKind::Sub, CarryFlag::Calculate)
        }
    };
    arithmetic_flags(code, binary, carry, 31);
}

pub(super) fn unary_word(code: &mut InstructionSink<'_>, kind: UnaryKind, destination: Register32) {
    word_value(code, WordValue::Register(destination));
    code.local_set(LHS);
    let (binary, carry) = match kind {
        UnaryKind::Not => {
            code.local_get(LHS)
                .i32_const(0xffff)
                .i32_xor()
                .local_get(register(destination))
                .i32_const(!0xffff)
                .i32_and()
                .i32_or()
                .local_set(register(destination));
            return;
        }
        UnaryKind::Inc | UnaryKind::Dec => {
            code.i32_const(1).local_set(RHS);
            (
                if kind == UnaryKind::Inc {
                    BinaryKind::Add
                } else {
                    BinaryKind::Sub
                },
                CarryFlag::Preserve,
            )
        }
        UnaryKind::Neg => {
            code.local_get(LHS)
                .local_set(RHS)
                .i32_const(0)
                .local_set(LHS);
            (BinaryKind::Sub, CarryFlag::Calculate)
        }
    };
    code.local_get(LHS).local_get(RHS);
    if binary == BinaryKind::Add {
        code.i32_add();
    } else {
        code.i32_sub();
    }
    code.i32_const(0xffff).i32_and().local_set(RESULT);
    arithmetic_flags(code, binary, carry, 15);
    code.local_get(RESULT)
        .local_get(register(destination))
        .i32_const(!0xffff)
        .i32_and()
        .i32_or()
        .local_set(register(destination));
}

pub(super) fn unary_byte(
    code: &mut InstructionSink<'_>,
    kind: UnaryKind,
    destination: ByteRegister,
) {
    byte_value(code, ByteValue::Register(destination));
    code.local_set(LHS);
    let (binary, carry) = match kind {
        UnaryKind::Not => {
            code.local_get(LHS).i32_const(0xff).i32_xor();
            insert_byte(code, destination);
            return;
        }
        UnaryKind::Inc | UnaryKind::Dec => {
            code.i32_const(1).local_set(RHS);
            (
                if kind == UnaryKind::Inc {
                    BinaryKind::Add
                } else {
                    BinaryKind::Sub
                },
                CarryFlag::Preserve,
            )
        }
        UnaryKind::Neg => {
            code.local_get(LHS)
                .local_set(RHS)
                .i32_const(0)
                .local_set(LHS);
            (BinaryKind::Sub, CarryFlag::Calculate)
        }
    };
    code.local_get(LHS).local_get(RHS);
    if binary == BinaryKind::Add {
        code.i32_add();
    } else {
        code.i32_sub();
    }
    code.i32_const(0xff).i32_and().local_set(RESULT);
    arithmetic_flags(code, binary, carry, 7);
    code.local_get(RESULT);
    insert_byte(code, destination);
}

pub(super) fn unary(code: &mut InstructionSink<'_>, kind: UnaryKind, destination: Register32) {
    code.local_get(register(destination)).local_set(LHS);
    let (binary, carry) = match kind {
        UnaryKind::Not => {
            code.local_get(LHS)
                .i32_const(-1)
                .i32_xor()
                .local_set(register(destination));
            return;
        }
        UnaryKind::Inc | UnaryKind::Dec => {
            code.i32_const(1).local_set(RHS);
            (
                if kind == UnaryKind::Inc {
                    BinaryKind::Add
                } else {
                    BinaryKind::Sub
                },
                CarryFlag::Preserve,
            )
        }
        UnaryKind::Neg => {
            code.local_get(LHS)
                .local_set(RHS)
                .i32_const(0)
                .local_set(LHS);
            (BinaryKind::Sub, CarryFlag::Calculate)
        }
    };
    code.local_get(LHS).local_get(RHS);
    if binary == BinaryKind::Add {
        code.i32_add();
    } else {
        code.i32_sub();
    }
    code.local_set(RESULT);
    arithmetic_flags(code, binary, carry, 31);
    code.local_get(RESULT).local_set(register(destination));
}
