use super::*;

#[test]
fn activation_wait_has_a_stable_literal_kind_and_full_width_owner_identity() {
    let mut bytes = [0xab; MEDIA_RECORD_SIZE];
    encode_media_record(
        MediaRecordKind::ActivationWait,
        0x1122334455667788,
        [0x100000002, 1, 0, 0, 0],
        &mut bytes,
    );
    assert_eq!(
        &bytes[..16],
        &[82, 51, 77, 82, 1, 0, 22, 0, 64, 0, 0, 0, 0, 0, 0, 0]
    );
    assert_eq!(
        &bytes[16..32],
        &[
            0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11, 2, 0, 0, 0, 1, 0, 0, 0
        ]
    );
    assert_eq!(&bytes[32..40], &1u64.to_le_bytes());
    assert!(bytes[40..].iter().all(|byte| *byte == 0));
}
