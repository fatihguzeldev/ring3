use ring3_core::{
    FileOffset, PeCertificateEntry, PeCertificateEntryError, PeCertificateEntryTable,
    PeCertificateError, PeCertificateTable, PeKind, parse_pe_certificate_entries,
};

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn fixed(plus: bool) -> usize {
    if plus { 112 } else { 96 }
}

fn fixture(plus: bool, table: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0; 512 + table.len()];
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 60, 128);
    bytes[128..132].copy_from_slice(b"PE\0\0");
    let size = u16::try_from(fixed(plus) + 40).unwrap();
    bytes[148..150].copy_from_slice(&size.to_le_bytes());
    bytes[152..154].copy_from_slice(&if plus { 0x20b_u16 } else { 0x10b }.to_le_bytes());
    put32(&mut bytes, 152 + fixed(plus) - 4, 5);
    put32(&mut bytes, 152 + fixed(plus) + 32, 512);
    put32(
        &mut bytes,
        152 + fixed(plus) + 36,
        u32::try_from(table.len()).unwrap(),
    );
    bytes[512..].copy_from_slice(table);
    bytes
}

#[test]
fn ordered_entries_preserve_raw_fields_and_borrow_body_and_alignment_bytes() {
    let raw = [
        11, 0, 0, 0, 0, 2, 2, 0, 1, 2, 3, 0xaa, 0xbb, 0xcc, 0xdd, 0xee, 8, 0, 0, 0, 0xff, 0xff,
        0x34, 0x12,
    ];
    for plus in [false, true] {
        let bytes = fixture(plus, &raw);
        let before = bytes.clone();
        let result = parse_pe_certificate_entries(&bytes).unwrap().unwrap();
        assert_eq!(
            result,
            PeCertificateEntryTable {
                table: PeCertificateTable {
                    kind: if plus { PeKind::Pe32Plus } else { PeKind::Pe32 },
                    file_offset: FileOffset::new(512),
                    size: 24,
                    raw_bytes: &raw,
                },
                entries: vec![
                    PeCertificateEntry {
                        file_offset: FileOffset::new(512),
                        length: 11,
                        revision: 0x200,
                        certificate_type: 2,
                        raw_body: &[1, 2, 3],
                        alignment_padding: &[0xaa, 0xbb, 0xcc, 0xdd, 0xee],
                    },
                    PeCertificateEntry {
                        file_offset: FileOffset::new(528),
                        length: 8,
                        revision: 0xffff,
                        certificate_type: 0x1234,
                        raw_body: &[],
                        alignment_padding: &[],
                    },
                ],
            }
        );
        assert_eq!(result.table.raw_bytes.as_ptr(), bytes[512..].as_ptr());
        assert_eq!(result.entries[0].raw_body.as_ptr(), bytes[520..].as_ptr());
        assert_eq!(
            result.entries[0].alignment_padding.as_ptr(),
            bytes[523..].as_ptr()
        );
        assert_eq!(result.entries[1].raw_body.as_ptr(), bytes[536..].as_ptr());
        assert_eq!(bytes, before);
    }
}

#[test]
fn declared_body_includes_zeros_and_duplicate_unknown_metadata() {
    let raw = [16, 0, 0, 0, 0xff, 0xff, 0xfe, 0xff, 1, 2, 3, 0, 0, 0, 0, 0];
    for plus in [false, true] {
        let bytes = fixture(plus, &raw.repeat(2));
        let result = parse_pe_certificate_entries(&bytes).unwrap().unwrap();
        assert_eq!(result.entries.len(), 2);
        for (entry, offset) in result.entries.iter().zip([512, 528]) {
            assert_eq!(entry.file_offset, FileOffset::new(offset));
            assert_eq!((entry.revision, entry.certificate_type), (0xffff, 0xfffe));
            assert_eq!(entry.raw_body, &[1, 2, 3, 0, 0, 0, 0, 0]);
            assert!(entry.alignment_padding.is_empty());
        }
    }
}

#[test]
fn every_short_length_is_refused_at_its_original_entry_index() {
    for plus in [false, true] {
        for index in 0..2 {
            for length in 0..8 {
                let mut raw = [8, 0, 0, 0, 0, 2, 2, 0].repeat(2);
                put32(&mut raw, usize::from(index) * 8, length);
                assert_eq!(
                    parse_pe_certificate_entries(&fixture(plus, &raw)),
                    Err(PeCertificateEntryError::InvalidEntryLength {
                        entry_index: index,
                        length
                    })
                );
            }
        }
    }
}

#[test]
fn oversize_and_rounded_u32_overflow_lengths_never_wrap() {
    for plus in [false, true] {
        for (length, padded_length) in [
            (9, 16),
            (16, 16),
            (17, 24),
            (0xffff_fff8, 0xffff_fff8),
            (0xffff_fff9, 0x1_0000_0000),
            (u32::MAX, 0x1_0000_0000),
        ] {
            let mut raw = [8, 0, 0, 0, 0, 2, 2, 0].repeat(2);
            put32(&mut raw, 8, length);
            assert_eq!(
                parse_pe_certificate_entries(&fixture(plus, &raw)),
                Err(PeCertificateEntryError::EntryExceedsTable {
                    entry_index: 1,
                    length,
                    padded_length,
                    remaining: 8,
                })
            );
        }
    }
}

#[test]
fn exact_entry_budget_succeeds_and_next_header_is_not_read() {
    for plus in [false, true] {
        let mut raw = [8, 0, 0, 0, 0, 2, 2, 0].repeat(256);
        let bytes = fixture(plus, &raw);
        let result = parse_pe_certificate_entries(&bytes).unwrap().unwrap();
        assert_eq!(result.entries.len(), 256);
        assert_eq!(result.entries[255].file_offset, FileOffset::new(2552));
        for length in [0, 8, u32::MAX] {
            raw.resize(257 * 8, 0);
            put32(&mut raw, 256 * 8, length);
            assert_eq!(
                parse_pe_certificate_entries(&fixture(plus, &raw)),
                Err(PeCertificateEntryError::EntryLimitExceeded {
                    entry_index: 256,
                    limit: 256
                })
            );
        }
    }
}

#[test]
fn trailing_zero_header_is_an_invalid_record_not_a_terminator() {
    for plus in [false, true] {
        let raw = [8, 0, 0, 0, 0, 2, 2, 0];
        assert_eq!(
            parse_pe_certificate_entries(&fixture(plus, &raw))
                .unwrap()
                .unwrap()
                .entries
                .len(),
            1
        );
        let mut extended = raw.to_vec();
        extended.extend_from_slice(&[0; 8]);
        assert_eq!(
            parse_pe_certificate_entries(&fixture(plus, &extended)),
            Err(PeCertificateEntryError::InvalidEntryLength {
                entry_index: 1,
                length: 0
            })
        );
    }
}

#[test]
fn raw_table_absence_and_errors_precede_record_inspection() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, &[0; 8]);
        put32(&mut bytes, 152 + fixed(plus) + 32, 513);
        assert_eq!(
            parse_pe_certificate_entries(&bytes),
            Err(PeCertificateEntryError::Table(
                PeCertificateError::UnalignedTableOffset {
                    file_offset: FileOffset::new(513),
                }
            ))
        );
        put32(&mut bytes, 152 + fixed(plus) - 4, 4);
        assert_eq!(parse_pe_certificate_entries(&bytes), Ok(None));
        put32(&mut bytes, 152 + fixed(plus) - 4, 5);
        put32(&mut bytes, 152 + fixed(plus) + 32, 0);
        put32(&mut bytes, 152 + fixed(plus) + 36, 0);
        assert_eq!(parse_pe_certificate_entries(&bytes), Ok(None));
    }
}

#[test]
fn rounded_steps_keep_following_headers_and_borrowed_slices_exact() {
    for plus in [false, true] {
        for (length, step) in [
            (8, 8),
            (9, 16),
            (15, 16),
            (16, 16),
            (17, 24),
            (23, 24),
            (24, 24),
            (31, 32),
        ] {
            let mut raw = vec![0x42; length];
            put32(&mut raw, 0, u32::try_from(length).unwrap());
            raw.resize(step, 0x99);
            raw.extend_from_slice(&[8, 0, 0, 0, 0, 1, 9, 0]);
            let bytes = fixture(plus, &raw);
            let result = parse_pe_certificate_entries(&bytes).unwrap().unwrap();
            assert_eq!(result.entries.len(), 2);
            assert_eq!(result.entries[0].raw_body, &raw[8..length]);
            assert_eq!(result.entries[0].alignment_padding, &raw[length..step]);
            assert_eq!(result.entries[0].raw_body.as_ptr(), bytes[520..].as_ptr());
            assert_eq!(
                result.entries[0].alignment_padding.as_ptr(),
                bytes[512 + length..].as_ptr()
            );
            assert_eq!(
                result.entries[1].file_offset,
                FileOffset::new(u64::try_from(512 + step).unwrap())
            );
            assert_eq!(
                (
                    result.entries[1].revision,
                    result.entries[1].certificate_type
                ),
                (0x100, 9)
            );
        }
    }
}

fn generated_fixture_with_synthetic_entries(variable: &str, plus: bool) {
    let path = std::env::var_os(variable).expect("explicit generated fixture path");
    let original = std::fs::read(&path).unwrap();
    assert_eq!(original.len(), 1024);
    assert_eq!(&original[60..64], &120_u32.to_le_bytes());
    let slot = if plus { 288 } else { 272 };
    assert_eq!(&original[slot..slot + 8], &[0; 8]);
    let raw = [
        13, 0, 0, 0, 0, 2, 0xff, 0xff, b'r', b'i', b'n', b'g', b'3', 0, 0, 0, 8, 0, 0, 0, 0, 1, 2,
        0,
    ];
    // synthetic record framing only; this does not create a signed image.
    let mut bytes = original.clone();
    bytes.extend_from_slice(&raw);
    put32(&mut bytes, slot, 1024);
    put32(&mut bytes, slot + 4, 24);
    let before = bytes.clone();
    let result = parse_pe_certificate_entries(&bytes).unwrap().unwrap();
    assert_eq!(
        result,
        PeCertificateEntryTable {
            table: PeCertificateTable {
                kind: if plus { PeKind::Pe32Plus } else { PeKind::Pe32 },
                file_offset: FileOffset::new(1024),
                size: 24,
                raw_bytes: &raw,
            },
            entries: vec![
                PeCertificateEntry {
                    file_offset: FileOffset::new(1024),
                    length: 13,
                    revision: 0x200,
                    certificate_type: 0xffff,
                    raw_body: b"ring3",
                    alignment_padding: &[0; 3],
                },
                PeCertificateEntry {
                    file_offset: FileOffset::new(1040),
                    length: 8,
                    revision: 0x100,
                    certificate_type: 2,
                    raw_body: &[],
                    alignment_padding: &[],
                },
            ],
        }
    );
    assert_eq!(result.table.raw_bytes.as_ptr(), bytes[1024..].as_ptr());
    assert_eq!(result.entries[0].raw_body.as_ptr(), bytes[1032..].as_ptr());
    assert_eq!(
        result.entries[0].alignment_padding.as_ptr(),
        bytes[1037..].as_ptr()
    );
    assert_eq!(result.entries[1].raw_body.as_ptr(), bytes[1048..].as_ptr());
    assert_eq!(bytes, before);
    assert_eq!(std::fs::read(path).unwrap(), original);
}

#[test]
#[ignore = "requires an explicit generated fixture path"]
fn generated_pe32_with_synthetic_certificate_entries_matches_file_metadata() {
    generated_fixture_with_synthetic_entries("RING3_PE32_FIXTURE", false);
}

#[test]
#[ignore = "requires an explicit generated fixture path"]
fn generated_pe32plus_with_synthetic_certificate_entries_matches_file_metadata() {
    generated_fixture_with_synthetic_entries("RING3_PE32PLUS_FIXTURE", true);
}
