use super::*;

fn words(words: &[u16]) -> Vec<u8> {
    words.iter().flat_map(|word| word.to_le_bytes()).collect()
}

#[test]
fn standard_popup_omits_id_and_end_bits_close_only_their_own_lists() {
    let bytes = words(&[
        0,
        2,
        0xbeef,
        0x10,
        b'F' as u16,
        0,
        0x10,
        b'N' as u16,
        0,
        0x88,
        37,
        b'X' as u16,
        0,
        0x81,
        38,
        b'Y' as u16,
        0,
        0x80,
        39,
        0xd83d,
        0xde00,
        0,
    ]);
    let parsed = Template32::parse(&bytes, 5, 3).unwrap();
    assert_eq!(parsed.length, bytes.len());
    assert_eq!(parsed.help, 0);
    assert_eq!(parsed.items.len(), 5);
    assert_eq!(
        parsed
            .items
            .iter()
            .map(|item| item.parent)
            .collect::<Vec<_>>(),
        [None, Some(0), Some(1), Some(0), None]
    );
    assert_eq!(
        parsed.items.iter().map(|item| item.id).collect::<Vec<_>>(),
        [0, 0, 37, 38, 39]
    );
    assert!(parsed.items[0].popup && parsed.items[1].popup);
    assert_eq!(parsed.items[2].state, 8);
    assert_eq!(parsed.items[3].state, 1);
    assert_eq!(parsed.items[4].text, [0xd83d, 0xde00]);
}

#[test]
fn extended_alignment_preserves_popup_ids_help_and_full_item_fields() {
    let bytes = words(&[
        1,
        4,
        0x5678,
        0x1234,
        0x200,
        0,
        3,
        0,
        0x4321,
        0x8765,
        0x81,
        b'P' as u16,
        0,
        0xabcd,
        0x2468,
        0x1357,
        0x100,
        0,
        8,
        0,
        0xbeef,
        0xcafe,
        0x80,
        b'Z' as u16,
        0,
    ]);
    let parsed = Template32::parse(&bytes, 2, 2).unwrap();
    assert_eq!(parsed.length, bytes.len());
    assert_eq!(parsed.help, 0x12345678);
    assert_eq!(parsed.items[0].id, 0x87654321);
    assert_eq!(parsed.items[0].kind, 0x200);
    assert_eq!(parsed.items[0].state, 3);
    assert_eq!(parsed.items[0].help, 0x13572468);
    assert_eq!(parsed.items[1].parent, Some(0));
    assert_eq!(parsed.items[1].kind, 0x100);
    assert_eq!(parsed.items[1].state, 8);
    assert_eq!(parsed.items[1].id, 0xcafebeef);
}

#[test]
fn exact_reader_never_fetches_trailing_or_missing_template_bytes() {
    let bytes = words(&[0, 0, 0x80, 9, b'A' as u16, 0]);
    for length in 0..bytes.len() {
        assert_eq!(
            Template32::parse(&bytes[..length], 1, 1),
            Err(E::InvalidRequest)
        );
    }
    let mut accessed = 0;
    let parsed = Template32::read(
        |offset, output| {
            let end = offset + output.len();
            assert!(end <= bytes.len());
            output.copy_from_slice(&bytes[offset..end]);
            accessed = accessed.max(end);
            Ok(())
        },
        bytes.len() + 4096,
        1,
        1,
    )
    .unwrap();
    assert_eq!(parsed.length, bytes.len());
    assert_eq!(accessed, bytes.len());
}

#[test]
fn limits_and_invalid_extended_flags_refuse_before_unbounded_descent() {
    let bytes = words(&[0, 0, 0x90, 0, 0x80, 1, 0]);
    assert_eq!(Template32::parse(&bytes, 2, 1), Err(E::Capacity));
    assert_eq!(Template32::parse(&bytes, 1, 2), Err(E::Capacity));
    assert_eq!(Template32::parse(&bytes, 2, 2).unwrap().items.len(), 2);
    let bad = words(&[1, 4, 0, 0, 0, 0, 0, 0, 1, 0, 0x82, 0]);
    assert_eq!(Template32::parse(&bad, 1, 1), Err(E::InvalidRequest));
    assert_eq!(
        Template32::parse(&words(&[1, 0]), 1, 1),
        Err(E::InvalidRequest)
    );
    assert_eq!(
        Template32::parse(&words(&[2, 0]), 1, 1),
        Err(E::InvalidRequest)
    );
}
