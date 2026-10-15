use crate::cpu::dbt::wasm::instructions::simd::direct::{P, S, arithmetic};
use wasm_encoder::{BlockType, InstructionSink, ValType};
pub(super) fn lanes(
    code: &mut InstructionSink<'_>,
    kind: P,
    lane_bytes: u8,
    load_source: impl Fn(&mut InstructionSink<'_>),
    load_count: impl Fn(&mut InstructionSink<'_>),
) {
    let bits = i32::from(lane_bytes) * 8;
    load_count(code);
    code.i64_const(i64::from(bits))
        .i64_ge_u()
        .if_(BlockType::Result(ValType::V128));
    if kind == P::ShiftRightSigned {
        load_source(code);
        code.i32_const(bits - 1);
        arithmetic(code, kind, lane_bytes);
    } else {
        code.v128_const(0);
    }
    code.else_();
    load_source(code);
    load_count(code);
    code.i32_wrap_i64();
    arithmetic(code, kind, lane_bytes);
    code.end();
}

pub(super) fn bytes(
    code: &mut InstructionSink<'_>,
    kind: S,
    immediate: u8,
    load_source: impl FnOnce(&mut InstructionSink<'_>),
) {
    let count = usize::from(immediate).min(16);
    let lanes = std::array::from_fn(|i| {
        if kind == S::ShiftBytesLeft {
            if i >= count { (i - count) as u8 } else { 16 }
        } else if i + count < 16 {
            (i + count) as u8
        } else {
            16
        }
    });
    load_source(code);
    code.v128_const(0).i8x16_shuffle(lanes);
}
