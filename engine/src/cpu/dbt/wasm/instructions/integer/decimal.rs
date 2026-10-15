use crate::cpu::dbt::wasm::instructions::integer::{
    AsciiAdjustKind, BlockType, ByteRegister, ByteValue, DecimalAdjustKind, FLAGS, InstructionSink,
    LHS, RESULT, RHS, Register32, WordValue, byte_value, divide_error_if, insert_byte,
    logical_flags, parity_flag, register, word_value, zero_sign_flags,
};
pub(super) fn ascii_adjust_after_multiplication(
    code: &mut InstructionSink<'_>,
    base: u8,
    exit_depth: u32,
) {
    if base == 0 {
        code.i32_const(1);
        divide_error_if(code, exit_depth);
        return;
    }
    byte_value(code, ByteValue::Register(ByteRegister::Al));
    code.local_tee(LHS)
        .i32_const(i32::from(base))
        .i32_div_u()
        .local_set(RHS)
        .local_get(LHS)
        .i32_const(i32::from(base))
        .i32_rem_u()
        .local_set(RESULT);
    logical_flags(code, 7);
    code.local_get(register(Register32::Eax))
        .i32_const(!0xffff)
        .i32_and()
        .local_get(RHS)
        .i32_const(8)
        .i32_shl()
        .i32_or()
        .local_get(RESULT)
        .i32_or()
        .local_set(register(Register32::Eax));
}

pub(super) fn ascii_adjust_before_division(code: &mut InstructionSink<'_>, base: u8) {
    byte_value(code, ByteValue::Register(ByteRegister::Al));
    byte_value(code, ByteValue::Register(ByteRegister::Ah));
    code.i32_const(i32::from(base))
        .i32_mul()
        .i32_add()
        .i32_const(0xff)
        .i32_and()
        .local_set(RESULT);
    logical_flags(code, 7);
    code.local_get(register(Register32::Eax))
        .i32_const(!0xffff)
        .i32_and()
        .local_get(RESULT)
        .i32_or()
        .local_set(register(Register32::Eax));
}

pub(super) fn ascii_adjust(code: &mut InstructionSink<'_>, kind: AsciiAdjustKind) {
    word_value(code, WordValue::Register(Register32::Eax));
    code.local_set(RESULT)
        .local_get(FLAGS)
        .local_tee(RHS)
        .i32_const(!0x8d5)
        .i32_and()
        .local_set(FLAGS)
        .local_get(RESULT)
        .i32_const(0xf)
        .i32_and()
        .i32_const(9)
        .i32_gt_u()
        .local_get(RHS)
        .i32_const(0x10)
        .i32_and()
        .i32_or()
        .if_(BlockType::Empty)
        .local_get(RESULT)
        .i32_const(0x106);
    if kind == AsciiAdjustKind::Add {
        code.i32_add();
    } else {
        code.i32_sub();
    }
    code.local_set(RESULT)
        .local_get(FLAGS)
        .i32_const(0x11)
        .i32_or()
        .local_set(FLAGS)
        .end()
        .local_get(register(Register32::Eax))
        .i32_const(!0xffff)
        .i32_and()
        .local_get(RESULT)
        .i32_const(0xff0f)
        .i32_and()
        .i32_or()
        .local_set(register(Register32::Eax));
}

pub(super) fn decimal_adjust(code: &mut InstructionSink<'_>, kind: DecimalAdjustKind) {
    byte_value(code, ByteValue::Register(ByteRegister::Al));
    code.local_tee(LHS)
        .local_set(RESULT)
        .local_get(FLAGS)
        .local_tee(RHS)
        .i32_const(!0x8d5)
        .i32_and()
        .local_set(FLAGS)
        .local_get(LHS)
        .i32_const(0xf)
        .i32_and()
        .i32_const(9)
        .i32_gt_u()
        .local_get(RHS)
        .i32_const(0x10)
        .i32_and()
        .i32_or()
        .if_(BlockType::Empty)
        .local_get(RESULT)
        .i32_const(6);
    if kind == DecimalAdjustKind::Add {
        code.i32_add();
    } else {
        code.i32_sub();
    }
    code.local_set(RESULT)
        .local_get(FLAGS)
        .i32_const(0x10)
        .i32_or()
        .local_set(FLAGS);
    if kind == DecimalAdjustKind::Subtract {
        code.local_get(FLAGS)
            .local_get(LHS)
            .i32_const(6)
            .i32_lt_u()
            .i32_or()
            .local_set(FLAGS);
    }
    code.end()
        .local_get(LHS)
        .i32_const(0x99)
        .i32_gt_u()
        .local_get(RHS)
        .i32_const(1)
        .i32_and()
        .i32_or()
        .if_(BlockType::Empty)
        .local_get(RESULT)
        .i32_const(0x60);
    if kind == DecimalAdjustKind::Add {
        code.i32_add();
    } else {
        code.i32_sub();
    }
    code.local_set(RESULT)
        .local_get(FLAGS)
        .i32_const(1)
        .i32_or()
        .local_set(FLAGS)
        .end()
        .local_get(RESULT)
        .i32_const(0xff)
        .i32_and()
        .local_set(RESULT)
        .local_get(FLAGS);
    parity_flag(code);
    zero_sign_flags(code, 7);
    code.local_set(FLAGS).local_get(RESULT);
    insert_byte(code, ByteRegister::Al);
}
