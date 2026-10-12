use super::{BinaryKind, CarryFlag, FLAGS, InstructionSink, LHS, RESULT, RHS};

pub(super) fn arithmetic_flags(
    code: &mut InstructionSink<'_>,
    kind: BinaryKind,
    carry: CarryFlag,
    sign_bit: i32,
) {
    code.local_get(FLAGS)
        .i32_const(if matches!(carry, CarryFlag::Preserve) {
            0x401
        } else {
            0x400
        })
        .i32_and()
        .i32_const(2)
        .i32_or();
    if matches!(carry, CarryFlag::Calculate) {
        if matches!(kind, BinaryKind::Add | BinaryKind::Adc) {
            code.local_get(RESULT).local_get(LHS).i32_lt_u();
        } else {
            code.local_get(LHS).local_get(RHS).i32_lt_u();
        }
        if matches!(kind, BinaryKind::Adc | BinaryKind::Sbb) {
            code.local_get(FLAGS).i32_const(1).i32_and();
            if kind == BinaryKind::Adc {
                code.local_get(RESULT).local_get(LHS).i32_eq();
            } else {
                code.local_get(LHS).local_get(RHS).i32_eq();
            }
            code.i32_and().i32_or();
        }
        code.i32_or();
    }
    parity_flag(code);
    code.local_get(LHS)
        .local_get(RHS)
        .i32_xor()
        .local_get(RESULT)
        .i32_xor()
        .i32_const(0x10)
        .i32_and()
        .i32_or();
    zero_sign_flags(code, sign_bit);
    code.local_get(LHS).local_get(RHS).i32_xor();
    if matches!(kind, BinaryKind::Add | BinaryKind::Adc) {
        code.i32_const(-1).i32_xor();
    }
    code.local_get(LHS)
        .local_get(RESULT)
        .i32_xor()
        .i32_and()
        .i32_const(sign_bit)
        .i32_shr_u()
        .i32_const(11)
        .i32_shl()
        .i32_or()
        .local_set(FLAGS);
}

pub(super) fn logical_flags(code: &mut InstructionSink<'_>, sign_bit: i32) {
    // af is undefined on x86; this profile deterministically clears it with cf/of.
    code.local_get(FLAGS)
        .i32_const(0x400)
        .i32_and()
        .i32_const(2)
        .i32_or();
    parity_flag(code);
    zero_sign_flags(code, sign_bit);
    code.local_set(FLAGS);
}

pub(super) fn parity_flag(code: &mut InstructionSink<'_>) {
    code.local_get(RESULT)
        .i32_const(0xff)
        .i32_and()
        .i32_popcnt()
        .i32_const(1)
        .i32_and()
        .i32_eqz()
        .i32_const(2)
        .i32_shl()
        .i32_or();
}

pub(super) fn zero_sign_flags(code: &mut InstructionSink<'_>, sign_bit: i32) {
    code.local_get(RESULT)
        .i32_eqz()
        .i32_const(6)
        .i32_shl()
        .i32_or()
        .local_get(RESULT)
        .i32_const(sign_bit - 7)
        .i32_shr_u()
        .i32_const(0x80)
        .i32_and()
        .i32_or();
}
