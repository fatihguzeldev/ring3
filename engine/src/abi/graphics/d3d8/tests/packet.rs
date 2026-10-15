use super::*;

fn encoded() -> Vec<u8> {
    let mut bytes = vec![0; 64];
    append_command(&Command::CursorVisible(true), 9, &mut bytes).unwrap();
    write_batch_header(
        &mut bytes,
        BatchHeader {
            length: 100,
            fragments: 1,
            epoch: 0x1122_3344_5566_7788,
            sequence: 7,
            first_command: 9,
            last_command: 9,
        },
    )
    .unwrap();
    bytes
}

#[test]
fn packet_header_and_fragment_fields_match_literal_little_endian_contract() {
    let bytes = encoded();
    assert_eq!(
        &bytes[..64],
        &[
            0x52, 0x33, 0x44, 0x38, 2, 0, 8, 0, 100, 0, 0, 0, 1, 0, 0, 0, 0x88, 0x77, 0x66, 0x55,
            0x44, 0x33, 0x22, 0x11, 7, 0, 0, 0, 0, 0, 0, 0, 9, 0, 0, 0, 0, 0, 0, 0, 9, 0, 0, 0, 0,
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        ]
    );
    assert_eq!(
        &bytes[64..96],
        &[
            19, 0, 3, 0, 36, 0, 0, 0, 9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 0
        ]
    );
    assert_eq!(&bytes[96..], &[1, 0, 0, 0]);
    let (header, fragments) = packet(&bytes).unwrap();
    assert_eq!(header.sequence, 7);
    assert_eq!(fragments.len(), 1);
}

#[test]
fn packet_reader_rejects_reserved_flags_lengths_versions_and_command_identity() {
    for (offset, value) in [
        (4, 1),
        (48, 1),
        (88, 1),
        (66, 7),
        (80, 1),
        (72, 0),
        (12, 2),
        (64, 0),
    ] {
        let mut bytes = encoded();
        bytes[offset] = value;
        assert!(packet(&bytes).is_err(), "offset {offset}");
    }
    let mut bytes = encoded();
    bytes.pop();
    assert!(packet(&bytes).is_err());
}
