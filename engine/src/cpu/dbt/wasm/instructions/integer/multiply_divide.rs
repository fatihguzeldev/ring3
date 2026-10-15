use crate::cpu::dbt::wasm::{
    imports::Imports,
    instructions::integer::{
        BlockType, ByteRegister, ByteValue, DivideKind, ExtensionKind, FLAGS, InstructionSink, LHS,
        Location32, MEMORY_BYTES, MultiplyKind, REASON, RESULT, RHS, Register32, SmallWidth,
        byte_value, extend_value, memory, register,
    },
};
pub(super) fn word_accumulator_multiply(
    code: &mut InstructionSink<'_>,
    kind: MultiplyKind,
    source: u32,
) {
    let extension = match kind {
        MultiplyKind::Unsigned => ExtensionKind::Zero,
        MultiplyKind::Signed => ExtensionKind::Sign,
    };
    code.local_get(register(Register32::Eax));
    extend_value(code, extension, SmallWidth::Word);
    code.local_set(LHS).local_get(source);
    extend_value(code, extension, SmallWidth::Word);
    code.local_set(RHS)
        .local_get(LHS)
        .local_get(RHS)
        .i32_mul()
        .local_set(RESULT)
        .local_get(FLAGS)
        .i32_const(!0x8d5)
        .i32_and()
        .local_get(RESULT);
    match kind {
        MultiplyKind::Unsigned => {
            code.i32_const(16).i32_shr_u().i32_const(0).i32_ne();
        }
        MultiplyKind::Signed => {
            code.local_get(RESULT);
            extend_value(code, ExtensionKind::Sign, SmallWidth::Word);
            code.i32_ne();
        }
    }
    code.i32_const(0x801)
        .i32_mul()
        .i32_or()
        .local_set(FLAGS)
        .local_get(register(Register32::Eax))
        .i32_const(!0xffff)
        .i32_and()
        .local_get(RESULT)
        .i32_const(0xffff)
        .i32_and()
        .i32_or()
        .local_set(register(Register32::Eax))
        .local_get(register(Register32::Edx))
        .i32_const(!0xffff)
        .i32_and()
        .local_get(RESULT)
        .i32_const(16)
        .i32_shr_u()
        .i32_or()
        .local_set(register(Register32::Edx));
}

pub(super) fn byte_accumulator_multiply(
    code: &mut InstructionSink<'_>,
    kind: MultiplyKind,
    source: ByteRegister,
) {
    byte_value(code, ByteValue::Register(ByteRegister::Al));
    code.local_set(LHS);
    byte_value(code, ByteValue::Register(source));
    code.local_set(RHS);
    byte_accumulator_multiply_value(code, kind);
}

pub(super) fn byte_accumulator_multiply_value(code: &mut InstructionSink<'_>, kind: MultiplyKind) {
    for operand in [LHS, RHS] {
        code.local_get(operand);
        if kind == MultiplyKind::Signed {
            extend_value(code, ExtensionKind::Sign, SmallWidth::Byte);
        }
    }
    code.i32_mul()
        .local_set(RESULT)
        .local_get(register(Register32::Eax))
        .i32_const(!0xffff)
        .i32_and()
        .local_get(RESULT)
        .i32_const(0xffff)
        .i32_and()
        .i32_or()
        .local_set(register(Register32::Eax))
        .local_get(FLAGS)
        .i32_const(!0x8d5)
        .i32_and()
        .local_get(RESULT);
    match kind {
        MultiplyKind::Unsigned => {
            code.i32_const(0xff).i32_gt_u();
        }
        MultiplyKind::Signed => {
            code.local_get(RESULT);
            extend_value(code, ExtensionKind::Sign, SmallWidth::Byte);
            code.i32_ne();
        }
    }
    code.i32_const(0x801).i32_mul().i32_or().local_set(FLAGS);
}

pub(super) fn accumulator_multiply(
    code: &mut InstructionSink<'_>,
    kind: MultiplyKind,
    source: u32,
) {
    code.local_get(register(Register32::Eax))
        .local_set(LHS)
        .local_get(source)
        .local_set(RHS)
        .local_get(LHS)
        .local_get(RHS)
        .i32_mul()
        .local_set(register(Register32::Eax));
    for operand in [LHS, RHS] {
        code.local_get(operand);
        match kind {
            MultiplyKind::Unsigned => {
                code.i64_extend_i32_u();
            }
            MultiplyKind::Signed => {
                code.i64_extend_i32_s();
            }
        }
    }
    code.i64_mul()
        .i64_const(32)
        .i64_shr_u()
        .i32_wrap_i64()
        .local_set(register(Register32::Edx))
        .local_get(FLAGS)
        .i32_const(!0x8d5)
        .i32_and()
        .local_get(register(Register32::Edx));
    match kind {
        MultiplyKind::Unsigned => {
            code.i32_const(0);
        }
        MultiplyKind::Signed => {
            code.local_get(register(Register32::Eax))
                .i32_const(31)
                .i32_shr_s();
        }
    }
    code.i32_ne()
        .i32_const(0x801)
        .i32_mul()
        .i32_or()
        .local_set(FLAGS);
}

pub(super) fn word_accumulator_divide(
    code: &mut InstructionSink<'_>,
    kind: DivideKind,
    source: u32,
    exit_depth: u32,
) {
    code.local_get(source);
    extend_value(
        code,
        if kind == DivideKind::Signed {
            ExtensionKind::Sign
        } else {
            ExtensionKind::Zero
        },
        SmallWidth::Word,
    );
    code.local_set(RHS)
        .local_get(register(Register32::Edx))
        .i32_const(0xffff)
        .i32_and()
        .i32_const(16)
        .i32_shl()
        .local_get(register(Register32::Eax))
        .i32_const(0xffff)
        .i32_and()
        .i32_or()
        .local_set(LHS)
        .local_get(RHS)
        .i32_eqz();
    divide_error_if(code, exit_depth);
    if kind == DivideKind::Signed {
        // wasm traps on this signed division before the WORD quotient can be checked.
        code.local_get(LHS)
            .i32_const(i32::MIN)
            .i32_eq()
            .local_get(RHS)
            .i32_const(-1)
            .i32_eq()
            .i32_and();
        divide_error_if(code, exit_depth);
    }
    code.local_get(LHS).local_get(RHS);
    match kind {
        DivideKind::Unsigned => {
            code.i32_div_u();
        }
        DivideKind::Signed => {
            code.i32_div_s();
        }
    }
    code.local_set(RESULT).local_get(RESULT);
    match kind {
        DivideKind::Unsigned => {
            code.i32_const(0xffff).i32_gt_u();
        }
        DivideKind::Signed => {
            code.i32_const(i32::from(i16::MIN))
                .i32_lt_s()
                .local_get(RESULT)
                .i32_const(i32::from(i16::MAX))
                .i32_gt_s()
                .i32_or();
        }
    }
    divide_error_if(code, exit_depth);
    code.local_get(LHS)
        .local_get(RESULT)
        .local_get(RHS)
        .i32_mul()
        .i32_sub()
        .local_set(RHS)
        .local_get(register(Register32::Eax))
        .i32_const(!0xffff)
        .i32_and()
        .local_get(RESULT)
        .i32_const(0xffff)
        .i32_and()
        .i32_or()
        .local_set(register(Register32::Eax))
        .local_get(register(Register32::Edx))
        .i32_const(!0xffff)
        .i32_and()
        .local_get(RHS)
        .i32_const(0xffff)
        .i32_and()
        .i32_or()
        .local_set(register(Register32::Edx));
}

pub(super) fn byte_accumulator_divide(
    code: &mut InstructionSink<'_>,
    kind: DivideKind,
    source: ByteRegister,
    exit_depth: u32,
) {
    byte_value(code, ByteValue::Register(source));
    byte_accumulator_divide_value(code, kind, exit_depth);
}

pub(super) fn byte_accumulator_divide_value(
    code: &mut InstructionSink<'_>,
    kind: DivideKind,
    exit_depth: u32,
) {
    if kind == DivideKind::Signed {
        extend_value(code, ExtensionKind::Sign, SmallWidth::Byte);
    }
    code.local_set(RHS).local_get(register(Register32::Eax));
    extend_value(
        code,
        if kind == DivideKind::Signed {
            ExtensionKind::Sign
        } else {
            ExtensionKind::Zero
        },
        SmallWidth::Word,
    );
    code.local_set(LHS).local_get(RHS).i32_eqz();
    divide_error_if(code, exit_depth);
    code.local_get(LHS).local_get(RHS);
    match kind {
        DivideKind::Unsigned => {
            code.i32_div_u();
        }
        DivideKind::Signed => {
            code.i32_div_s();
        }
    }
    code.local_set(RESULT).local_get(RESULT);
    match kind {
        DivideKind::Unsigned => {
            code.i32_const(0xff).i32_gt_u();
        }
        DivideKind::Signed => {
            code.i32_const(-128)
                .i32_lt_s()
                .local_get(RESULT)
                .i32_const(127)
                .i32_gt_s()
                .i32_or();
        }
    }
    divide_error_if(code, exit_depth);
    code.local_get(LHS)
        .local_get(RESULT)
        .local_get(RHS)
        .i32_mul()
        .i32_sub()
        .local_set(RHS)
        .local_get(register(Register32::Eax))
        .i32_const(!0xffff)
        .i32_and()
        .local_get(RESULT)
        .i32_const(0xff)
        .i32_and()
        .i32_or()
        .local_get(RHS)
        .i32_const(0xff)
        .i32_and()
        .i32_const(8)
        .i32_shl()
        .i32_or()
        .local_set(register(Register32::Eax));
}

pub(super) fn accumulator_divide(
    code: &mut InstructionSink<'_>,
    kind: DivideKind,
    source: u32,
    exit_depth: u32,
) {
    code.local_get(source)
        .local_set(RHS)
        .local_get(RHS)
        .i32_eqz();
    divide_error_if(code, exit_depth);
    match kind {
        DivideKind::Unsigned => {
            code.local_get(register(Register32::Edx))
                .local_get(RHS)
                .i32_ge_u();
        }
        DivideKind::Signed => {
            code.local_get(register(Register32::Edx))
                .i32_const(i32::MIN)
                .i32_eq()
                .local_get(register(Register32::Eax))
                .i32_eqz()
                .i32_and()
                .local_get(RHS)
                .i32_const(-1)
                .i32_eq()
                .i32_and();
        }
    }
    divide_error_if(code, exit_depth);
    divide_operands(code, kind);
    match kind {
        DivideKind::Unsigned => {
            code.i64_div_u();
        }
        DivideKind::Signed => {
            code.i64_div_s();
        }
    }
    // preflight has finished using this i64 local; keep the quotient wide until validation.
    code.local_set(MEMORY_BYTES);
    if kind == DivideKind::Signed {
        code.local_get(MEMORY_BYTES)
            .i64_const(i64::from(i32::MIN))
            .i64_lt_s()
            .local_get(MEMORY_BYTES)
            .i64_const(i64::from(i32::MAX))
            .i64_gt_s()
            .i32_or();
        divide_error_if(code, exit_depth);
    }
    divide_operands(code, kind);
    code.local_get(MEMORY_BYTES)
        .i64_mul()
        .i64_sub()
        .i32_wrap_i64()
        .local_set(RESULT)
        .local_get(MEMORY_BYTES)
        .i32_wrap_i64()
        .local_set(register(Register32::Eax))
        .local_get(RESULT)
        .local_set(register(Register32::Edx));
}

fn divide_operands(code: &mut InstructionSink<'_>, kind: DivideKind) {
    code.local_get(register(Register32::Edx))
        .i64_extend_i32_u()
        .i64_const(32)
        .i64_shl()
        .local_get(register(Register32::Eax))
        .i64_extend_i32_u()
        .i64_or()
        .local_get(RHS);
    match kind {
        DivideKind::Unsigned => {
            code.i64_extend_i32_u();
        }
        DivideKind::Signed => {
            code.i64_extend_i32_s();
        }
    }
}

pub(super) fn divide_error_if(code: &mut InstructionSink<'_>, exit_depth: u32) {
    code.if_(BlockType::Empty)
        .i32_const(10)
        .local_set(REASON)
        .br(exit_depth + 1)
        .end();
}

pub(super) fn signed_multiply_word(
    code: &mut InstructionSink<'_>,
    destination: Register32,
    source: u32,
    immediate: Option<u16>,
) {
    match immediate {
        Some(immediate) => {
            code.local_get(source)
                .i32_const(16)
                .i32_shl()
                .i32_const(16)
                .i32_shr_s()
                .local_set(LHS)
                .i32_const(i32::from(immediate as i16))
                .local_set(RHS);
        }
        None => {
            code.local_get(register(destination))
                .i32_const(16)
                .i32_shl()
                .i32_const(16)
                .i32_shr_s()
                .local_set(LHS)
                .local_get(source)
                .i32_const(16)
                .i32_shl()
                .i32_const(16)
                .i32_shr_s()
                .local_set(RHS);
        }
    }
    code.local_get(LHS)
        .local_get(RHS)
        .i32_mul()
        .local_set(RESULT);
    // signed16 products fit signed32; overflow means truncation changes the signed value.
    code.local_get(FLAGS)
        .i32_const(!0x8d5)
        .i32_and()
        .local_get(RESULT)
        .local_get(RESULT)
        .i32_const(16)
        .i32_shl()
        .i32_const(16)
        .i32_shr_s()
        .i32_ne()
        .i32_const(0x801)
        .i32_mul()
        .i32_or()
        .local_set(FLAGS)
        .local_get(register(destination))
        .i32_const(!0xffff)
        .i32_and()
        .local_get(RESULT)
        .i32_const(0xffff)
        .i32_and()
        .i32_or()
        .local_set(register(destination));
}

pub(super) fn signed_multiply(
    code: &mut InstructionSink<'_>,
    destination: Register32,
    source: Location32,
    immediate: Option<u32>,
    imports: Imports,
    exit_depth: u32,
) {
    let source = match source {
        Location32::Register(source) => register(source),
        Location32::Memory(address) => {
            // helper validation uses operand scratch; capture operands after it succeeds.
            memory::load_result(code, address, imports, exit_depth);
            RESULT
        }
    };
    match immediate {
        Some(immediate) => {
            code.local_get(source)
                .local_set(LHS)
                .i32_const(immediate as i32)
                .local_set(RHS);
        }
        None => {
            code.local_get(register(destination))
                .local_set(LHS)
                .local_get(source)
                .local_set(RHS);
        }
    }
    code.local_get(LHS)
        .local_get(RHS)
        .i32_mul()
        .local_set(RESULT);
    // PF/AF/ZF/SF are undefined; this profile clears them.
    code.local_get(FLAGS)
        .i32_const(!0x8d5)
        .i32_and()
        .local_get(LHS)
        .i64_extend_i32_s()
        .local_get(RHS)
        .i64_extend_i32_s()
        .i64_mul()
        .local_get(RESULT)
        .i64_extend_i32_s()
        .i64_ne()
        .i32_const(0x801)
        .i32_mul()
        .i32_or()
        .local_set(FLAGS)
        .local_get(RESULT)
        .local_set(register(destination));
}
