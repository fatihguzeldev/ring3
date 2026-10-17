use super::*;

#[test]
fn composition_record_preserves_unaligned_attributes_clauses_and_utf16() {
    let mut bytes = vec![0; 43];
    header::write_version(&mut bytes, *b"R3IM", 1);
    for (at, value) in [(12, 2), (16, 1), (20, 1), (24, 2), (28, 1)] {
        header::write_u32(&mut bytes, at, value);
    }
    bytes[32..34].copy_from_slice(&0x65e5u16.to_le_bytes());
    bytes[34] = 7;
    bytes[39..43].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(
        decode_composition_event(&bytes),
        Ok(Event::Update(InputComposition32 {
            text: vec![0x65e5],
            attributes: vec![7],
            clauses: vec![0, 1],
            cursor: 1,
            ..Default::default()
        }))
    );
    for len in 0..bytes.len() {
        assert!(decode_composition_event(&bytes[..len]).is_err());
    }
    header::write_u32(&mut bytes, 16, u32::MAX);
    assert_eq!(decode_composition_event(&bytes), Err(AbiError::Length));
}

#[test]
fn begin_cancel_and_commit_reject_hidden_payload_and_invalid_metadata() {
    for kind in [1, 4] {
        let mut bytes = [0; 32];
        header::write_version(&mut bytes, *b"R3IM", 1);
        header::write_u32(&mut bytes, 12, kind);
        assert!(decode_composition_event(&bytes).is_ok());
        header::write_u32(&mut bytes, 28, 1);
        assert_eq!(decode_composition_event(&bytes), Err(AbiError::Flags));
    }
    let mut bytes = [0; 34];
    header::write_version(&mut bytes, *b"R3IM", 1);
    header::write_u32(&mut bytes, 12, 3);
    header::write_u32(&mut bytes, 16, 1);
    bytes[32..].copy_from_slice(&65u16.to_le_bytes());
    assert_eq!(
        decode_composition_event(&bytes),
        Ok(Event::Commit(vec![65]))
    );
}
