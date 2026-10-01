use wasm_encoder::InstructionSink;

use super::locals::FLAGS;
use crate::cpu::x86::ir::Condition;

pub(super) fn condition(code: &mut InstructionSink<'_>, condition: Condition) {
    use Condition::*;
    match condition {
        Overflow | NotOverflow => bit(code, 0x800),
        Below | AboveOrEqual => bit(code, 1),
        Equal | NotEqual => bit(code, 0x40),
        BelowOrEqual | Above => {
            bit(code, 1);
            bit(code, 0x40);
            code.i32_or();
        }
        Sign | NotSign => bit(code, 0x80),
        Parity | NotParity => bit(code, 4),
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
    if matches!(
        condition,
        NotOverflow
            | AboveOrEqual
            | NotEqual
            | Above
            | NotSign
            | NotParity
            | GreaterOrEqual
            | Greater
    ) {
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
