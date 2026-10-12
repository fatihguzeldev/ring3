use wasm_encoder::InstructionSink;

use super::locals::FLAGS;
use crate::cpu::x86::ir::Condition;

pub(super) fn condition(code: &mut InstructionSink<'_>, condition: Condition) {
    use Condition::*;
    match condition {
        Overflow => bit(code, 0x800),
        NotOverflow => bit_is_clear(code, 0x800),
        Below => bit(code, 1),
        AboveOrEqual => bit_is_clear(code, 1),
        Equal => bit(code, 0x40),
        NotEqual => bit_is_clear(code, 0x40),
        BelowOrEqual => bit(code, 0x41),
        Above => bit_is_clear(code, 0x41),
        Sign => bit(code, 0x80),
        NotSign => bit_is_clear(code, 0x80),
        Parity => bit(code, 4),
        NotParity => bit_is_clear(code, 4),
        Less | GreaterOrEqual | LessOrEqual | Greater => {
            bit(code, 0x80);
            bit(code, 0x800);
            code.i32_xor();
            if matches!(condition, LessOrEqual | Greater) {
                bit(code, 0x40);
                code.i32_or();
            }
        }
    }
    if matches!(condition, GreaterOrEqual | Greater) {
        code.i32_eqz();
    }
}

fn bit(code: &mut InstructionSink<'_>, mask: i32) {
    code.local_get(FLAGS)
        .i32_const(mask)
        .i32_and()
        .i32_const(0)
        .i32_ne();
}

fn bit_is_clear(code: &mut InstructionSink<'_>, mask: i32) {
    code.local_get(FLAGS).i32_const(mask).i32_and().i32_eqz();
}
