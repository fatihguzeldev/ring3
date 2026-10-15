use super::*;

#[test]
fn installation_retains_literal_header_reserved_words_and_full_unit_identity() {
    let mut bytes = [0xa5; INSTALLATION_RECORD_SIZE + 8];
    encode_installation(
        0x0123_4567_89ab_cdef,
        7,
        (&mut bytes[4..4 + INSTALLATION_RECORD_SIZE])
            .try_into()
            .unwrap(),
    );
    assert_eq!(
        &bytes[4..36],
        &[
            0x52, 0x33, 0x49, 0x4e, 1, 0, 1, 0, 32, 0, 0, 0, 0, 0, 0, 0, 0xef, 0xcd, 0xab, 0x89,
            0x67, 0x45, 0x23, 0x01, 7, 0, 0, 0, 0, 0, 0, 0,
        ]
    );
    assert_eq!(&bytes[..4], &[0xa5; 4]);
    assert_eq!(&bytes[36..], &[0xa5; 4]);
}

#[test]
fn dispatcher_retains_full_key_pointer_length_and_output_span() {
    let mut bytes = [0xa5; DISPATCHER_RECORD_SIZE + 8];
    encode_dispatcher(
        0xfedc_ba98_7654_3210,
        0x89ab_cdef,
        0x1234_5678,
        (&mut bytes[4..4 + DISPATCHER_RECORD_SIZE])
            .try_into()
            .unwrap(),
    );
    assert_eq!(
        &bytes[4..36],
        &[
            0x52, 0x33, 0x44, 0x50, 1, 0, 1, 0, 32, 0, 0, 0, 0, 0, 0, 0, 0x10, 0x32, 0x54, 0x76,
            0x98, 0xba, 0xdc, 0xfe, 0xef, 0xcd, 0xab, 0x89, 0x78, 0x56, 0x34, 0x12,
        ]
    );
    assert_eq!(&bytes[..4], &[0xa5; 4]);
    assert_eq!(&bytes[36..], &[0xa5; 4]);
}

#[test]
fn resident_module_keeps_its_original_unmagicked_twenty_four_byte_record() {
    let mut bytes = [0xa5; RESIDENT_MODULE_RECORD_SIZE + 8];
    encode_resident_module(
        0xfedc_ba98_7654_3210,
        0x89ab_cdef,
        0x1234_5678,
        (&mut bytes[4..4 + RESIDENT_MODULE_RECORD_SIZE])
            .try_into()
            .unwrap(),
    );
    assert_eq!(
        &bytes[4..28],
        &[
            1, 0, 0, 0, 24, 0, 0, 0, 0x10, 0x32, 0x54, 0x76, 0x98, 0xba, 0xdc, 0xfe, 0xef, 0xcd,
            0xab, 0x89, 0x78, 0x56, 0x34, 0x12,
        ]
    );
    assert_eq!(&bytes[..4], &[0xa5; 4]);
    assert_eq!(&bytes[28..], &[0xa5; 4]);
}
