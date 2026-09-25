use ring3_core::{
    FileOffset, PeDebugDirectoryError, PeDebugPayload, PeDebugPayloadError as Error,
    PeDebugPayloadRange, PeDebugPayloadTable, PeHeaderError, PeKind, PeRvaError,
    RelativeVirtualAddress, parse_pe_debug_directory, parse_pe_debug_payloads,
};

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn fixed(plus: bool) -> usize {
    if plus { 112 } else { 96 }
}

fn directory(bytes: &mut [u8], plus: bool, rva: u32, size: u32) {
    put32(bytes, 152 + fixed(plus) + 48, rva);
    put32(bytes, 152 + fixed(plus) + 52, size);
}

fn section(bytes: &mut [u8], plus: bool, rva: u32, size: u32) {
    let section = 152 + fixed(plus) + 56;
    for (offset, value) in [8, 12, 16, 20].into_iter().zip([size, rva, size, 512]) {
        put32(bytes, section + offset, value);
    }
}

fn fixture(plus: bool) -> Vec<u8> {
    let mut bytes = vec![0; 1024];
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 60, 128);
    bytes[128..132].copy_from_slice(b"PE\0\0");
    bytes[134..136].copy_from_slice(&1_u16.to_le_bytes());
    bytes[148..150].copy_from_slice(&u16::try_from(fixed(plus) + 56).unwrap().to_le_bytes());
    bytes[152..154].copy_from_slice(&if plus { 0x20b_u16 } else { 0x10b }.to_le_bytes());
    put32(&mut bytes, 212, 512);
    put32(&mut bytes, 152 + fixed(plus) - 4, 7);
    section(&mut bytes, plus, 0x1000, 512);
    directory(&mut bytes, plus, 0x1000, 28);
    bytes
}

fn payload(bytes: &mut [u8], index: usize, file: u32, size: u32, rva: u32) {
    let base = 512 + index * 28;
    put32(bytes, base + 16, size);
    put32(bytes, base + 20, rva);
    put32(bytes, base + 24, file);
}

fn file_error(index: u16, offset: u32, size: u32, available: u64) -> Error {
    Error::PayloadRange {
        entry_index: index,
        file_offset: FileOffset::new(u64::from(offset)),
        size,
        cause: PeHeaderError::OutOfBounds {
            offset: FileOffset::new(u64::from(offset)),
            needed: u64::from(size),
            available,
        },
    }
}

#[test]
fn both_widths_preserve_complete_metadata_and_borrowed_bytes() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        for (offset, value) in [
            (0, 0x1234_5678),
            (4, 0x90ab_cdef),
            (8, 0xabcd_1234),
            (12, 0xfeed_beef),
        ] {
            put32(&mut bytes, 512 + offset, value);
        }
        payload(&mut bytes, 0, 768, 4, 0x1100);
        bytes[768..772].copy_from_slice(&[0x12, 0x34, 0x56, 0x78]);
        let before = bytes.clone();
        let result = parse_pe_debug_payloads(&bytes).unwrap().unwrap();
        assert_eq!(
            result,
            PeDebugPayloadTable {
                directory_table: parse_pe_debug_directory(&bytes).unwrap().unwrap(),
                payloads: vec![PeDebugPayload {
                    entry_index: 0,
                    range: Some(PeDebugPayloadRange {
                        file_offset: FileOffset::new(768),
                        raw_bytes: &[0x12, 0x34, 0x56, 0x78]
                    })
                }]
            }
        );
        assert_eq!(
            result.directory_table.kind,
            if plus { PeKind::Pe32Plus } else { PeKind::Pe32 }
        );
        assert_eq!(
            result.payloads[0].range.unwrap().raw_bytes.as_ptr(),
            bytes[768..].as_ptr()
        );
        assert_eq!(parse_pe_debug_payloads(&bytes), Ok(Some(result)));
        assert_eq!(bytes, before);
    }
}

#[test]
fn absence_and_present_zero_records_remain_distinct() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        let table = parse_pe_debug_payloads(&bytes).unwrap().unwrap();
        assert_eq!(
            table.payloads,
            [PeDebugPayload {
                entry_index: 0,
                range: None
            }]
        );
        directory(&mut bytes, plus, 0, 0);
        assert_eq!(parse_pe_debug_payloads(&bytes), Ok(None));
        directory(&mut bytes, plus, u32::MAX, u32::MAX);
        put32(&mut bytes, 152 + fixed(plus) - 4, 6);
        assert_eq!(parse_pe_debug_payloads(&bytes), Ok(None));
    }
}

#[test]
fn zero_size_skips_invalid_file_and_rva_coordinates() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        payload(&mut bytes, 0, u32::MAX, 0, u32::MAX);
        let table = parse_pe_debug_payloads(&bytes).unwrap().unwrap();
        assert_eq!(table.payloads[0].range, None);
        assert_eq!(
            table.directory_table.entries[0].pointer_to_raw_data,
            FileOffset::new(u64::from(u32::MAX))
        );
        assert_eq!(
            table.directory_table.entries[0].address_of_raw_data,
            RelativeVirtualAddress::new(u32::MAX)
        );
    }
}

#[test]
fn raw_file_pointer_is_independent_of_rva_and_opaque_type() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        bytes[768..772].copy_from_slice(b"data");
        for rva in [0, 0x1100, 0x1104, 0x8000, u32::MAX] {
            for kind in [0, 2, 16, u32::MAX] {
                payload(&mut bytes, 0, 768, 4, rva);
                put32(&mut bytes, 524, kind);
                let result = parse_pe_debug_payloads(&bytes).unwrap().unwrap();
                assert_eq!(result.payloads[0].range.unwrap().raw_bytes, b"data");
                assert_eq!(result.directory_table.entries[0].debug_type, kind);
            }
        }
    }
}

#[test]
fn physical_header_overlay_and_region_crossings_are_readable() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        bytes.extend_from_slice(b"overlay!");
        for (file, size) in [(0, 2), (449, 3), (510, 4), (1022, 4), (1024, 8), (1027, 5)] {
            payload(&mut bytes, 0, file, size, 0);
            let result = parse_pe_debug_payloads(&bytes).unwrap().unwrap();
            let range = result.payloads[0].range.unwrap();
            let start = usize::try_from(file).unwrap();
            let end = start + usize::try_from(size).unwrap();
            assert_eq!(range.file_offset, FileOffset::new(u64::from(file)));
            assert_eq!(range.raw_bytes, &bytes[start..end]);
            assert_eq!(range.raw_bytes.as_ptr(), bytes[start..].as_ptr());
        }
    }
}

#[test]
fn physical_end_truncation_and_widened_file_ends_fail_exactly() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        bytes.extend_from_slice(b"data");
        payload(&mut bytes, 0, 1024, 4, 0);
        for length in 1024..1028 {
            assert_eq!(
                parse_pe_debug_payloads(&bytes[..length]),
                Err(file_error(
                    0,
                    1024,
                    4,
                    u64::try_from(length - 1024).unwrap()
                ))
            );
        }
        assert!(parse_pe_debug_payloads(&bytes).is_ok());
        for (file, size, available) in [
            (1028, 1, 0),
            (1029, 1, 0),
            (u32::MAX, 1, 0),
            (u32::MAX, u32::MAX, 0),
            (1027, 2, 1),
        ] {
            payload(&mut bytes, 0, file, size, 0);
            assert_eq!(
                parse_pe_debug_payloads(&bytes),
                Err(file_error(0, file, size, available))
            );
        }
    }
}

#[test]
fn first_nonempty_file_failure_retains_its_entry_index() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        directory(&mut bytes, plus, 0x1000, 112);
        payload(&mut bytes, 0, u32::MAX, 0, 0);
        payload(&mut bytes, 1, 768, 4, 0);
        payload(&mut bytes, 2, 1023, 2, 0);
        payload(&mut bytes, 3, u32::MAX, u32::MAX, 0);
        assert_eq!(
            parse_pe_debug_payloads(&bytes),
            Err(file_error(2, 1023, 2, 1))
        );
        payload(&mut bytes, 2, 768, 4, 0);
        assert_eq!(
            parse_pe_debug_payloads(&bytes),
            Err(file_error(3, u32::MAX, u32::MAX, 0))
        );
    }
}

#[test]
fn duplicate_and_overlapping_entries_keep_separate_views() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        directory(&mut bytes, plus, 0x1000, 84);
        bytes[768..774].copy_from_slice(b"abcdef");
        for (i, file) in [768, 768, 770].into_iter().enumerate() {
            payload(&mut bytes, i, file, 4, 0);
        }
        let result = parse_pe_debug_payloads(&bytes).unwrap().unwrap();
        assert_eq!(result.payloads.len(), 3);
        for (i, expected) in [b"abcd", b"abcd", b"cdef"].into_iter().enumerate() {
            assert_eq!(usize::from(result.payloads[i].entry_index), i);
            assert_eq!(result.payloads[i].range.unwrap().raw_bytes, expected);
        }
        assert_eq!(result.payloads[0].range, result.payloads[1].range);
    }
}

#[test]
fn maximum_256_alias_views_share_input_without_copying() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        bytes.resize(7680 + 65_536, 0x5a);
        section(&mut bytes, plus, 0x1000, 7168);
        directory(&mut bytes, plus, 0x1000, 7168);
        for i in 0..256 {
            payload(&mut bytes, i, 7680, 65_536, 0);
        }
        let table = parse_pe_debug_payloads(&bytes).unwrap().unwrap();
        assert_eq!(table.payloads.len(), 256);
        for (i, view) in table.payloads.iter().enumerate() {
            assert_eq!(usize::from(view.entry_index), i);
            let range = view.range.unwrap();
            assert_eq!(range.raw_bytes.len(), 65_536);
            assert_eq!(range.raw_bytes.as_ptr(), bytes[7680..].as_ptr());
        }
        directory(&mut bytes, plus, 0x1000, 7196);
        assert_eq!(
            parse_pe_debug_payloads(&bytes),
            Err(Error::Directory(
                PeDebugDirectoryError::EntryLimitExceeded {
                    count: 257,
                    limit: 256
                }
            ))
        );
    }
}

#[test]
fn all_directory_validation_precedes_invalid_payloads() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        payload(&mut bytes, 0, u32::MAX, u32::MAX, 0);
        for (rva, size) in [
            (0, 1),
            (1, 0),
            (u32::MAX, 28),
            (0x1000, 29),
            (0x1000, 7196),
            (0x11f0, 28),
        ] {
            directory(&mut bytes, plus, rva, size);
            let error = parse_pe_debug_directory(&bytes).unwrap_err();
            assert_eq!(
                parse_pe_debug_payloads(&bytes),
                Err(Error::Directory(error))
            );
        }
        directory(&mut bytes, plus, 0, 0);
        bytes[0] = 0;
        assert_eq!(
            parse_pe_debug_payloads(&bytes),
            Err(Error::Directory(PeDebugDirectoryError::Base(
                PeRvaError::Parse(PeHeaderError::InvalidDosSignature {
                    offset: FileOffset::new(0)
                })
            )))
        );
    }
}

fn generated_fixture_with_synthetic_debug_payload(variable: &str, plus: bool) {
    let path = std::env::var_os(variable).expect("explicit generated fixture path");
    let original = std::fs::read(&path).unwrap();
    assert_eq!(original.len(), 1024);
    assert_eq!(&original[60..64], &120_u32.to_le_bytes());
    let slot = if plus { 304 } else { 288 };
    assert_eq!(&original[slot..slot + 8], &[0; 8]);
    assert_eq!(&original[448..476], &[0; 28]);
    let mut bytes = original.clone();
    put32(&mut bytes, slot, 448);
    put32(&mut bytes, slot + 4, 28);
    put32(&mut bytes, 460, 0xfeed_beef);
    put32(&mut bytes, 464, 4);
    put32(&mut bytes, 468, 0);
    put32(&mut bytes, 472, 1024);
    bytes.extend_from_slice(&[0x12, 0x34, 0x56, 0x78]);
    for (index, (before, after)) in original.iter().zip(&bytes).enumerate() {
        if !(slot..slot + 8).contains(&index) && !(448..476).contains(&index) {
            assert_eq!(before, after, "unexpected mutation at {index}");
        }
    }
    let before = bytes.clone();
    let result = parse_pe_debug_payloads(&bytes).unwrap().unwrap();
    assert_eq!(
        result.directory_table.kind,
        if plus { PeKind::Pe32Plus } else { PeKind::Pe32 }
    );
    assert_eq!(
        result.directory_table.directory_rva,
        RelativeVirtualAddress::new(448)
    );
    assert_eq!(
        result.directory_table.directory_file_offset,
        FileOffset::new(448)
    );
    assert_eq!(result.directory_table.directory_size, 28);
    assert_eq!(result.directory_table.entries.len(), 1);
    let entry = result.directory_table.entries[0];
    assert_eq!(entry.entry_rva, RelativeVirtualAddress::new(448));
    assert_eq!(entry.entry_file_offset, FileOffset::new(448));
    assert_eq!(entry.characteristics, 0);
    assert_eq!(entry.time_date_stamp, 0);
    assert_eq!(entry.major_version, 0);
    assert_eq!(entry.minor_version, 0);
    assert_eq!(entry.debug_type, 0xfeed_beef);
    assert_eq!(entry.size_of_data, 4);
    assert_eq!(entry.address_of_raw_data, RelativeVirtualAddress::new(0));
    assert_eq!(entry.pointer_to_raw_data, FileOffset::new(1024));
    assert_eq!(
        result.payloads,
        [PeDebugPayload {
            entry_index: 0,
            range: Some(PeDebugPayloadRange {
                file_offset: FileOffset::new(1024),
                raw_bytes: &[0x12, 0x34, 0x56, 0x78]
            })
        }]
    );
    assert_eq!(
        result.payloads[0].range.unwrap().raw_bytes.as_ptr(),
        bytes[1024..].as_ptr()
    );
    assert_eq!(parse_pe_debug_payloads(&bytes), Ok(Some(result)));
    assert_eq!(bytes, before);
    assert_eq!(std::fs::read(path).unwrap(), original);
}

#[test]
#[ignore = "requires an explicit generated fixture path"]
fn generated_pe32_with_synthetic_debug_payload_matches_overlay_bytes() {
    generated_fixture_with_synthetic_debug_payload("RING3_PE32_FIXTURE", false);
}

#[test]
#[ignore = "requires an explicit generated fixture path"]
fn generated_pe32plus_with_synthetic_debug_payload_matches_overlay_bytes() {
    generated_fixture_with_synthetic_debug_payload("RING3_PE32PLUS_FIXTURE", true);
}

fn generated_debug_fixture_matches_linked_payloads(variable: &str, plus: bool) {
    let path = std::env::var_os(variable).expect("explicit generated debug fixture path");
    let bytes = std::fs::read(&path).unwrap();
    let before = bytes.clone();
    let guid = if plus {
        [
            0xa2, 0x4e, 0x62, 0x0a, 0x25, 0x14, 0x3a, 0xc4, 0x4c, 0x4c, 0x44, 0x20, 0x50, 0x44,
            0x42, 0x2e,
        ]
    } else {
        [
            0x0d, 0x1a, 0x99, 0xe9, 0x14, 0xc9, 0x8e, 0x58, 0x4c, 0x4c, 0x44, 0x20, 0x50, 0x44,
            0x42, 0x2e,
        ]
    };
    let mut payload = Vec::from(*b"RSDS");
    payload.extend_from_slice(&guid);
    payload.extend_from_slice(&1_u32.to_le_bytes());
    payload.extend_from_slice(b"ring3-debug.pdb\0");
    assert_eq!(payload.len(), 40);
    let table = parse_pe_debug_payloads(&bytes).unwrap().unwrap();
    assert_eq!(
        table.directory_table.kind,
        if plus { PeKind::Pe32Plus } else { PeKind::Pe32 }
    );
    assert_eq!(
        table.directory_table,
        parse_pe_debug_directory(&bytes).unwrap().unwrap()
    );
    assert_eq!(
        table.payloads,
        [
            PeDebugPayload {
                entry_index: 0,
                range: Some(PeDebugPayloadRange {
                    file_offset: FileOffset::new(1592),
                    raw_bytes: &payload
                })
            },
            PeDebugPayload {
                entry_index: 1,
                range: None
            },
        ]
    );
    assert_eq!(
        table.payloads[0].range.unwrap().raw_bytes.as_ptr(),
        bytes[1592..].as_ptr()
    );
    assert_eq!(parse_pe_debug_payloads(&bytes), Ok(Some(table)));
    assert_eq!(bytes, before);
    assert_eq!(std::fs::read(path).unwrap(), before);
}

#[test]
#[ignore = "requires an explicit generated debug fixture path"]
fn generated_pe32_debug_payloads_match_linked_bytes() {
    generated_debug_fixture_matches_linked_payloads("RING3_DEBUG_PE32_FIXTURE", false);
}

#[test]
#[ignore = "requires an explicit generated debug fixture path"]
fn generated_pe32plus_debug_payloads_match_linked_bytes() {
    generated_debug_fixture_matches_linked_payloads("RING3_DEBUG_PE32PLUS_FIXTURE", true);
}
