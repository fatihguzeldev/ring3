use crate::cpu::dbt::wasm::instructions::simd::direct::{P, arithmetic};
use wasm_encoder::{BlockType, InstructionSink};
pub(super) fn multiply_dword(
    code: &mut InstructionSink<'_>,
    signed: bool,
    load_source: impl Fn(&mut InstructionSink<'_>, bool),
) {
    for second in [false, true] {
        load_source(code, second);
        code.v128_const(0)
            .i8x16_shuffle([0, 1, 2, 3, 8, 9, 10, 11, 16, 17, 18, 19, 20, 21, 22, 23]);
    }
    if signed {
        code.i64x2_extmul_low_i32x4_s();
    } else {
        code.i64x2_extmul_low_i32x4_u();
    }
}

pub(super) fn sum_absolute_byte_differences(
    code: &mut InstructionSink<'_>,
    load_source: impl Fn(&mut InstructionSink<'_>, bool),
) {
    load_source(code, false);
    load_source(code, true);
    code.i8x16_max_u();
    load_source(code, false);
    load_source(code, true);
    code.i8x16_min_u()
        .i8x16_sub()
        .i16x8_extadd_pairwise_i8x16_u()
        .i32x4_extadd_pairwise_i16x8_u();
    // four-byte sums are at most 1020, so narrowing preserves them.
    code.v128_const(0)
        .i16x8_narrow_i32x4_u()
        .i32x4_extadd_pairwise_i16x8_u()
        .i64x2_extend_low_i32x4_u();
}

pub(super) fn multiply_high(
    code: &mut InstructionSink<'_>,
    kind: P,
    vector_bytes: u8,
    load_source: impl Fn(&mut InstructionSink<'_>, bool),
) {
    let signed = kind == P::MultiplyHighSigned;
    for high in [false, true] {
        if high && vector_bytes == 8 {
            code.v128_const(0);
            continue;
        }
        load_source(code, false);
        load_source(code, true);
        match (signed, high) {
            (true, false) => {
                code.i32x4_extmul_low_i16x8_s();
            }
            (true, true) => {
                code.i32x4_extmul_high_i16x8_s();
            }
            (false, false) => {
                code.i32x4_extmul_low_i16x8_u();
            }
            (false, true) => {
                code.i32x4_extmul_high_i16x8_u();
            }
        }
        code.i32_const(16);
        if signed {
            code.i32x4_shr_s();
        } else {
            code.i32x4_shr_u();
        }
    }
    if signed {
        code.i16x8_narrow_i32x4_s();
    } else {
        code.i16x8_narrow_i32x4_u();
    }
}

pub(super) fn horizontal(
    code: &mut InstructionSink<'_>,
    kind: P,
    vector_bytes: u8,
    lane_bytes: u8,
    load_source: impl Fn(&mut InstructionSink<'_>, bool),
) {
    let size = usize::from(vector_bytes);
    let width = usize::from(lane_bytes);
    for odd in [false, true] {
        load_source(code, false);
        load_source(code, true);
        let lanes = std::array::from_fn(|byte| {
            if byte >= size {
                return 15;
            }
            let lane = byte / width;
            let source = if lane < size / width / 2 { 0 } else { 16 };
            (source + (lane % (size / width / 2) * 2 + usize::from(odd)) * width + byte % width)
                as u8
        });
        code.i8x16_shuffle(lanes);
    }
    arithmetic(
        code,
        match kind {
            P::HorizontalAdd => P::Add,
            P::HorizontalSubtract => P::Subtract,
            P::HorizontalAddSignedSaturate => P::AddSignedSaturate,
            P::HorizontalSubtractSignedSaturate => P::SubtractSignedSaturate,
            _ => unreachable!(),
        },
        lane_bytes,
    );
}

pub(super) fn multiply_add_unsigned_signed_bytes(
    code: &mut InstructionSink<'_>,
    load_source: impl Fn(&mut InstructionSink<'_>, bool),
) {
    for high in [false, true] {
        load_source(code, false);
        if high {
            code.i16x8_extend_high_i8x16_u();
        } else {
            code.i16x8_extend_low_i8x16_u();
        }
        load_source(code, true);
        if high {
            code.i16x8_extend_high_i8x16_s();
        } else {
            code.i16x8_extend_low_i8x16_s();
        }
        code.i16x8_mul().i32x4_extadd_pairwise_i16x8_s();
    }
    code.i16x8_narrow_i32x4_s();
}

pub(super) fn multiply_high_round_signed(
    code: &mut InstructionSink<'_>,
    load_source: impl Fn(&mut InstructionSink<'_>, bool),
) {
    for high in [false, true] {
        load_source(code, false);
        load_source(code, true);
        if high {
            code.i32x4_extmul_high_i16x8_s();
        } else {
            code.i32x4_extmul_low_i16x8_s();
        }
        code.i32_const(0x4000)
            .i32x4_splat()
            .i32x4_add()
            .i32_const(15)
            .i32x4_shr_s();
    }
    // x86 wraps +32768; wasm's saturating q15 multiply would return +32767.
    code.i8x16_shuffle([0, 1, 4, 5, 8, 9, 12, 13, 16, 17, 20, 21, 24, 25, 28, 29]);
}

pub(super) fn multiple_sum_absolute_byte_differences(
    code: &mut InstructionSink<'_>,
    immediate: u8,
    load_source: impl Fn(&mut InstructionSink<'_>, bool),
) {
    let source = immediate & 3;
    let destination = immediate >> 2 & 1;
    for byte in 0..4 {
        for (second, start, sliding) in [(false, destination * 4, true), (true, source * 4, false)]
        {
            load_source(code, second);
            code.v128_const(0)
                .i8x16_shuffle(std::array::from_fn(|lane| {
                    if lane < 8 {
                        start + byte + if sliding { lane as u8 } else { 0 }
                    } else {
                        16
                    }
                }))
                .i16x8_extend_low_i8x16_u();
        }
        code.i16x8_sub().i16x8_abs();
        if byte != 0 {
            code.i16x8_add();
        }
    }
}

pub(super) fn minimum_position_unsigned_word(
    code: &mut InstructionSink<'_>,
    scratch: [u32; 3],
    load_source: impl Fn(&mut InstructionSink<'_>),
) {
    let [result, rhs, lhs] = scratch;

    load_source(code);
    code.i16x8_extract_lane_u(0)
        .local_set(result)
        .i32_const(0)
        .local_set(rhs);
    for lane in 1..8 {
        load_source(code);
        code.i16x8_extract_lane_u(lane)
            .local_tee(lhs)
            .local_get(result)
            .i32_lt_u()
            .if_(BlockType::Empty)
            .local_get(lhs)
            .local_set(result)
            .i32_const(i32::from(lane))
            .local_set(rhs)
            .end();
    }
    code.v128_const(0)
        .local_get(result)
        .i16x8_replace_lane(0)
        .local_get(rhs)
        .i16x8_replace_lane(1);
}
