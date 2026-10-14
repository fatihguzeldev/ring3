use super::*;

#[test]
fn supplied_controls_have_literal_roundtrip_and_reject_unowned_bits() {
    for nmi in [None, Some(false), Some(true)] {
        let state = TaskControlState32::new(0x8001_003f, 0x1234_5000, 0x18, 0x400, nmi).unwrap();
        let mut bytes = [0; TASK_CONTROLS_SIZE];
        encode_task_controls(state, &mut bytes).unwrap();
        assert_eq!(&bytes[..16], b"R3CR\x01\0\x01\0\x28\0\0\0\0\0\0\0");
        assert_eq!(
            &bytes[16..32],
            &[
                0x3f, 0, 1, 0x80, 0, 0x50, 0x34, 0x12, 0x18, 0, 0, 0, 0, 4, 0, 0
            ]
        );
        assert_eq!(decode_task_controls(&bytes), Ok(state));
        for (offset, value) in [
            (16, 0x31_u32 | 0x40),
            (24, 4),
            (24, 0x10000),
            (28, 1),
            (32, 3),
            (36, 1),
        ] {
            let mut bad = bytes;
            bad[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            assert_eq!(decode_task_controls(&bad), Err(AbiError::TaskControls));
        }
    }
}
