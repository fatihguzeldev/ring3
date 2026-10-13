use super::S;
use wasm_encoder::InstructionSink;

pub(super) fn shuffle(
    code: &mut InstructionSink<'_>,
    kind: S,
    vector_bytes: u8,
    lane_bytes: u8,
    immediate: u8,
) {
    let width = usize::from(lane_bytes);
    let size = usize::from(vector_bytes);
    let mut lanes = std::array::from_fn(|i| i as u8);
    match kind {
        S::DuplicateEven | S::DuplicateOdd => {
            for lane in 0..size / width {
                let source = (lane / 2 * 2 + usize::from(kind == S::DuplicateOdd)) * width;
                for byte in 0..width {
                    lanes[lane * width + byte] = (source + byte) as u8;
                }
            }
        }
        S::BlendImmediate => {
            for lane in 0..size / width {
                for byte in 0..width {
                    lanes[lane * width + byte] =
                        (lane * width + byte + if immediate & (1 << lane) != 0 { 16 } else { 0 })
                            as u8;
                }
            }
        }
        S::Dwords | S::Words => {
            for i in 0..4 {
                for j in 0..width {
                    lanes[i * width + j] =
                        (((immediate >> (i * 2)) & 3) as usize * width + j) as u8;
                }
            }
        }
        S::HighWords | S::LowWords => {
            let start = if kind == S::HighWords { 8 } else { 0 };
            for i in 0..4 {
                for j in 0..2 {
                    lanes[start + i * 2 + j] =
                        (start + usize::from((immediate >> (i * 2)) & 3) * 2 + j) as u8;
                }
            }
        }
        S::Singles => {
            for i in 0..4 {
                for j in 0..4 {
                    lanes[i * 4 + j] = (usize::from((immediate >> (i * 2)) & 3) * 4
                        + j
                        + if i < 2 { 0 } else { 16 }) as u8;
                }
            }
        }
        S::Doubles => {
            for j in 0..8 {
                lanes[j] = (usize::from(immediate & 1) * 8 + j) as u8;
                lanes[8 + j] = (16 + usize::from((immediate >> 1) & 1) * 8 + j) as u8;
            }
        }
        S::UnpackLow | S::UnpackHigh => {
            let start = if kind == S::UnpackHigh { size / 2 } else { 0 };
            for i in 0..size / width / 2 {
                for j in 0..width {
                    lanes[i * width * 2 + j] = (start + i * width + j) as u8;
                    lanes[i * width * 2 + width + j] = (16 + start + i * width + j) as u8;
                }
            }
        }
        S::MoveLowToHigh => {
            for j in 0..8 {
                lanes[8 + j] = (16 + j) as u8;
            }
        }
        S::MoveHighToLow => {
            for (j, lane) in lanes[..8].iter_mut().enumerate() {
                *lane = (24 + j) as u8;
            }
        }
        _ => unreachable!(),
    }
    code.i8x16_shuffle(lanes);
}

pub(super) fn pack(code: &mut InstructionSink<'_>, kind: S, lane_bytes: u8) {
    match (kind, lane_bytes) {
        (S::PackSigned, 1) => {
            code.i8x16_narrow_i16x8_s();
        }
        (S::PackSigned, 2) => {
            code.i16x8_narrow_i32x4_s();
        }
        (S::PackUnsigned, 1) => {
            code.i8x16_narrow_i16x8_u();
        }
        (S::PackUnsigned, 2) => {
            code.i16x8_narrow_i32x4_u();
        }
        _ => unreachable!("direct saturating pack kind and width were decoded"),
    }
}

pub(super) fn widen(code: &mut InstructionSink<'_>, kind: S, lane_bytes: u8) {
    let (mut from, signed) = kind.widening().unwrap();
    while from < lane_bytes {
        match (from, signed) {
            (1, true) => {
                code.i16x8_extend_low_i8x16_s();
            }
            (1, false) => {
                code.i16x8_extend_low_i8x16_u();
            }
            (2, true) => {
                code.i32x4_extend_low_i16x8_s();
            }
            (2, false) => {
                code.i32x4_extend_low_i16x8_u();
            }
            (4, true) => {
                code.i64x2_extend_low_i32x4_s();
            }
            (4, false) => {
                code.i64x2_extend_low_i32x4_u();
            }
            _ => unreachable!("direct widening source width was decoded"),
        }
        from *= 2;
    }
}

pub(super) fn blend_variable(code: &mut InstructionSink<'_>, lane_bytes: u8) {
    code.i32_const(i32::from(lane_bytes * 8 - 1));
    match lane_bytes {
        1 => {
            code.i8x16_shr_s();
        }
        4 => {
            code.i32x4_shr_s();
        }
        8 => {
            code.i64x2_shr_s();
        }
        _ => unreachable!("direct variable blend width was decoded"),
    }
    code.v128_bitselect();
}

pub(super) fn bytes(
    code: &mut InstructionSink<'_>,
    vector_bytes: u8,
    reload_second: impl FnOnce(&mut InstructionSink<'_>),
) {
    code.i32_const(i32::from(vector_bytes) - 1)
        .i8x16_splat()
        .v128_and()
        .i8x16_swizzle();
    reload_second(code);
    code.i32_const(0).i8x16_splat().i8x16_ge_s().v128_and();
}

pub(super) fn align_right(code: &mut InstructionSink<'_>, size: usize, count: usize) {
    let lanes = std::array::from_fn(|byte| {
        let index = count + byte;
        if index < size {
            index as u8
        } else if index < size * 2 {
            (16 + index - size) as u8
        } else {
            0
        }
    });
    code.i8x16_shuffle(lanes);
    let mask = std::array::from_fn::<_, 16, _>(|byte| {
        if byte < size && count + byte < size * 2 {
            255u8
        } else {
            0
        }
    });
    code.v128_const(i128::from_le_bytes(mask)).v128_and();
}
