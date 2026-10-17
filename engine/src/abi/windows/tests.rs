use super::*;

#[test]
fn nls_callback_uses_additive_provider_seven_and_three_literal_arguments() {
    let record = WindowsCallbackRecord32 {
        provider: 7,
        procedure: 0x401000,
        count: 3,
        continuation: 0x100000002,
        arguments: [
            0x69f00000, 0x21, 0xdeadbeef, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ],
    };
    let mut bytes = [0; WINDOWS_CALLBACK_RECORD_SIZE];
    encode_windows_callback(Some(record), &mut bytes).unwrap();
    assert_eq!(&bytes[12..24], &[7, 0, 0, 0, 0, 0x10, 0x40, 0, 3, 0, 0, 0]);
    assert_eq!(
        &bytes[24..44],
        &[
            2, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0xf0, 0x69, 0x21, 0, 0, 0, 0xef, 0xbe, 0xad, 0xde
        ]
    );
    assert_eq!(decode_windows_callback(&bytes).unwrap(), Some(record));
    bytes[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(
        decode_windows_callback(&bytes),
        Err(AbiError::CallbackFrame)
    );
}

#[test]
fn windows_callback_carries_sixteen_arguments_and_rejects_reserved_tail() {
    let record = WindowsCallbackRecord32 {
        provider: 2,
        procedure: 0x12345678,
        count: 16,
        continuation: 0x1122334455667788,
        arguments: std::array::from_fn(|index| index as u32 + 1),
    };
    let mut bytes = [0; 96];
    encode_windows_callback(Some(record), &mut bytes).unwrap();
    assert_eq!(
        &bytes[..16],
        &[82, 51, 87, 67, 1, 0, 1, 0, 96, 0, 0, 0, 2, 0, 0, 0]
    );
    assert_eq!(decode_windows_callback(&bytes), Ok(Some(record)));
    bytes[20..24].copy_from_slice(&4u32.to_le_bytes());
    assert_eq!(decode_windows_callback(&bytes), Err(AbiError::Reserved));
    encode_windows_callback(None, &mut bytes).unwrap();
    assert_eq!(decode_windows_callback(&bytes), Ok(None));
    bytes[95] = 1;
    assert_eq!(decode_windows_callback(&bytes), Err(AbiError::Reserved));
}

#[test]
fn known_callback_providers_keep_the_wire_shape_and_reject_unknown_providers() {
    let mut bytes = [0; WINDOWS_CALLBACK_RECORD_SIZE];
    for provider in [3, 4, 5, 6, 7, 8, 9, 10] {
        let record = WindowsCallbackRecord32 {
            provider,
            procedure: 0x1234,
            count: 2,
            continuation: 9,
            arguments: std::array::from_fn(|index| if index < 2 { 17 } else { 0 }),
        };
        encode_windows_callback(Some(record), &mut bytes).unwrap();
        assert_eq!(decode_windows_callback(&bytes), Ok(Some(record)));
        bytes[12..16].copy_from_slice(&11u32.to_le_bytes());
        assert_eq!(
            decode_windows_callback(&bytes),
            Err(AbiError::CallbackFrame)
        );
    }
}

#[test]
fn files_progress_callback_has_literal_provider_and_thirteen_stack_words() {
    let record = WindowsCallbackRecord32 {
        provider: 5,
        procedure: 0x12345678,
        count: 13,
        continuation: 0x1122334455667788,
        arguments: [
            8, 0, 0, 0, 8, 0, 0, 0, 1, 1, 0x80000000, 0x80000004, 0xfeedbeef, 0, 0, 0,
        ],
    };
    let mut bytes = [0; 96];
    encode_windows_callback(Some(record), &mut bytes).unwrap();
    assert_eq!(
        &bytes[..32],
        &[
            82, 51, 87, 67, 1, 0, 1, 0, 96, 0, 0, 0, 5, 0, 0, 0, 0x78, 0x56, 0x34, 0x12, 13, 0, 0,
            0, 0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11
        ]
    );
    assert_eq!(
        &bytes[64..84],
        &[
            1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0x80, 4, 0, 0, 0x80, 0xef, 0xbe, 0xed, 0xfe
        ]
    );
    assert_eq!(&bytes[84..], &[0; 12]);
    assert_eq!(decode_windows_callback(&bytes), Ok(Some(record)));
    bytes[12..16].copy_from_slice(&11u32.to_le_bytes());
    assert_eq!(
        decode_windows_callback(&bytes),
        Err(AbiError::CallbackFrame)
    );
}
