use super::*;

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
