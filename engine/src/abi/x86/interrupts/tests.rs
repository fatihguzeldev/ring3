use super::*;

fn literal(limit: u16) -> [u8; INTERRUPT_DESCRIPTOR_TABLE_SIZE] {
    let mut bytes = [0; INTERRUPT_DESCRIPTOR_TABLE_SIZE];
    bytes[..16].copy_from_slice(&[82, 51, 73, 68, 1, 0, 1, 0, 20, 8, 0, 0, 0, 0, 0, 0]);
    bytes[16..18].copy_from_slice(&limit.to_le_bytes());
    for (index, byte) in bytes[20..].iter_mut().enumerate() {
        *byte = index.wrapping_mul(37).wrapping_add(0xa5) as u8;
    }
    bytes
}

#[test]
fn exact_literal_envelope_preserves_all_raw_slots_and_any_limit() {
    assert_eq!(INTERRUPT_DESCRIPTOR_TABLE_SIZE, 2068);
    for limit in [0, 1, 7, 8, u16::MAX] {
        let bytes = literal(limit);
        let table = decode_interrupt_descriptor_table(&bytes).unwrap();
        assert_eq!(table.limit(), limit);
        for vector in 0..=u8::MAX {
            let start = 20 + usize::from(vector) * 8;
            assert_eq!(table.record(vector).to_bytes(), bytes[start..start + 8]);
        }
        let mut output = [0xa5; INTERRUPT_DESCRIPTOR_TABLE_SIZE];
        encode_interrupt_descriptor_table(&table, &mut output).unwrap();
        assert_eq!(output, bytes);
    }
    let table = InterruptDescriptorTable32::new(
        0,
        Box::new([SegmentDescriptorRecord::from_words(0, 0); 256]),
    );
    let mut output = [0xa5; INTERRUPT_DESCRIPTOR_TABLE_SIZE];
    encode_interrupt_descriptor_table(&table, &mut output).unwrap();
    assert_eq!(decode_interrupt_descriptor_table(&output).unwrap(), table);
    assert!(output[16..].iter().all(|byte| *byte == 0));
}

#[test]
fn only_header_reserved_and_exact_size_constrain_the_wire() {
    let bytes = literal(u16::MAX);
    for offset in (0..16).chain([18, 19]) {
        let mut bad = bytes;
        bad[offset] ^= 0x80;
        assert!(
            decode_interrupt_descriptor_table(&bad).is_err(),
            "offset={offset}"
        );
    }
    for length in [0, 16, INTERRUPT_DESCRIPTOR_TABLE_SIZE - 1] {
        assert_eq!(
            decode_interrupt_descriptor_table(&bytes[..length]),
            Err(AbiError::Length)
        );
    }
    let mut oversized = bytes.to_vec();
    oversized.push(0);
    assert_eq!(
        decode_interrupt_descriptor_table(&oversized),
        Err(AbiError::Length)
    );
    let table = decode_interrupt_descriptor_table(&bytes).unwrap();
    let mut short = [0xa5; INTERRUPT_DESCRIPTOR_TABLE_SIZE - 1];
    assert_eq!(
        encode_interrupt_descriptor_table(&table, &mut short),
        Err(AbiError::Length)
    );
    assert_eq!(short, [0xa5; INTERRUPT_DESCRIPTOR_TABLE_SIZE - 1]);
    // descriptor payload bits, including reserved or impossible gates, are not wire errors.
    for offset in [
        20,
        27,
        20 + 128 * 8 + 5,
        INTERRUPT_DESCRIPTOR_TABLE_SIZE - 1,
    ] {
        let mut changed = bytes;
        changed[offset] ^= 0xff;
        let table = decode_interrupt_descriptor_table(&changed).unwrap();
        let mut roundtrip = [0; INTERRUPT_DESCRIPTOR_TABLE_SIZE];
        encode_interrupt_descriptor_table(&table, &mut roundtrip).unwrap();
        assert_eq!(roundtrip, changed);
    }
}
