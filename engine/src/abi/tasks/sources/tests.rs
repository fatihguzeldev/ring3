use super::*;

#[test]
fn cached_task_source_wire_preserves_selector_raw_descriptor_and_explicit_extent() {
    let current = TaskRegister::from_cached(
        0x4b,
        SegmentDescriptorRecord::from_words(0xffff0067, 0xff008bff),
    )
    .unwrap();
    let mut bytes = [0xa5; 32];
    encode_task_return_sources(current, 0x5f, &mut bytes).unwrap();
    assert_eq!(
        bytes,
        [
            82, 51, 84, 82, 1, 0, 1, 0, 32, 0, 0, 0, 0, 0, 0, 0, 75, 0, 0, 0, 95, 0, 0, 0, 103, 0,
            255, 255, 255, 139, 0, 255
        ]
    );
    assert_eq!(decode_task_return_sources(&bytes), Ok((current, 0x5f)));
    let mut short = [0xa5; 31];
    assert_eq!(
        encode_task_return_sources(current, 0, &mut short),
        Err(AbiError::Length)
    );
    assert_eq!(short, [0xa5; 31]);
}

#[test]
fn malformed_width_header_and_non_cached_tss_images_never_supply_task_authority() {
    let current = TaskRegister::from_cached(
        0x48,
        SegmentDescriptorRecord::from_words(0x50000067, 0x8b00),
    )
    .unwrap();
    let mut bytes = [0; 32];
    encode_task_return_sources(current, 0xffff, &mut bytes).unwrap();
    for offset in [0, 4, 6, 8, 12, 18, 22] {
        let mut bad = bytes;
        bad[offset] ^= 1;
        assert!(decode_task_return_sources(&bad).is_err(), "offset={offset}");
    }
    for (offset, value) in [
        (16, 0_u32),
        (16, 0x4c),
        (28, 0x8900),
        (28, 0x0b00),
        (28, 0x9b00),
    ] {
        let mut bad = bytes;
        bad[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        assert!(decode_task_return_sources(&bad).is_err());
    }
    assert_eq!(
        decode_task_return_sources(&bytes[..31]),
        Err(AbiError::Length)
    );
}
