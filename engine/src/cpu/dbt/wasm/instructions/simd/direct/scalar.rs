use wasm_encoder::InstructionSink;
pub(super) fn insert(
    code: &mut InstructionSink<'_>,
    lane_bytes: u8,
    immediate: u8,
    vector_bytes: u8,
) {
    let lane = immediate % (vector_bytes / lane_bytes);
    match lane_bytes {
        1 => {
            code.i8x16_replace_lane(lane);
        }
        2 => {
            code.i16x8_replace_lane(lane);
        }
        4 => {
            code.i32x4_replace_lane(lane);
        }
        _ => unreachable!("direct insertion width was decoded"),
    }
}

pub(super) fn extract(
    code: &mut InstructionSink<'_>,
    lane_bytes: u8,
    immediate: u8,
    vector_bytes: u8,
) {
    let lane = immediate % (vector_bytes / lane_bytes);
    match lane_bytes {
        1 => {
            code.i8x16_extract_lane_u(lane);
        }
        2 => {
            code.i16x8_extract_lane_u(lane);
        }
        4 => {
            code.i32x4_extract_lane(lane);
        }
        _ => unreachable!("validated scalar width"),
    }
}
pub(super) fn mask_bytes(code: &mut InstructionSink<'_>) {
    code.i8x16_bitmask();
}
