use super::*;
use crate::graphics::d3d8::{
    commands::PipelineId,
    state::{DeviceState, Matrix},
};

#[test]
fn complete_pipeline_snapshot_uses_identity_default_mask_with_literal_byte_budget() {
    let state = DeviceState::new(16, 16, false).unwrap();
    let command = Command::Pipeline {
        id: PipelineId(7),
        state: state.snapshot(),
    };
    assert_eq!(payload_size(&command), Ok((6, 3608)));
    let mut bytes = Vec::with_capacity(3608);
    let pointer = bytes.as_ptr();
    append_payload(&command, &mut bytes).unwrap();
    assert_eq!(bytes.as_ptr(), pointer);
    assert_eq!(&bytes[8..12], &2u32.to_le_bytes());
    assert_eq!(&bytes[1676..1684], &[10, 1, 0, 0, 9, 0, 0, 0]);
    assert_eq!(&bytes[1684..1720], &[0; 36]);
}

#[test]
fn sparse_transform_snapshot_preserves_negative_zero_and_last_palette_matrix() {
    let mut state = DeviceState::new(16, 16, false).unwrap();
    let mut negative = Matrix::IDENTITY;
    negative.0[1] = 0x8000_0000;
    state.set_transform(2, negative).unwrap();
    let mut last = Matrix::IDENTITY;
    last.0[0] = 2f32.to_bits();
    state.set_transform(511, last).unwrap();
    let command = Command::Pipeline {
        id: PipelineId(7),
        state: state.snapshot(),
    };
    assert_eq!(payload_size(&command), Ok((6, 3736)));
    let mut bytes = Vec::new();
    append_payload(&command, &mut bytes).unwrap();
    assert_eq!(&bytes[1684..1688], &1u32.to_le_bytes());
    assert_eq!(&bytes[1716..1720], &512u32.to_le_bytes());
    assert_eq!(&bytes[1724..1728], &[0, 0, 0, 128]);
    assert_eq!(&bytes[1784..1788], &2f32.to_bits().to_le_bytes());
    assert_eq!(20592 - bytes.len(), 16856);
}
