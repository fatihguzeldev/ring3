use ring3_core::{
    FileOffset, PeDebugDirectoryEntry, PeDebugDirectoryError, PeDebugDirectoryTable, PeHeaderError,
    PeKind, PeRvaError, RelativeVirtualAddress, parse_pe_debug_directory,
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

fn record(bytes: &mut [u8], offset: usize) {
    for (field, value) in [0, 4, 12, 16, 20, 24].into_iter().zip([
        0x1234_5678,
        0x90ab_cdef,
        0xfeed_beef,
        u32::MAX,
        0xffff_fffd,
        0xffff_fffe,
    ]) {
        put32(bytes, offset + field, value);
    }
    bytes[offset + 8..offset + 12].copy_from_slice(&[0x34, 0x12, 0xcd, 0xab]);
}

fn expected(rva: u32, file_offset: u64) -> PeDebugDirectoryEntry {
    PeDebugDirectoryEntry {
        entry_rva: RelativeVirtualAddress::new(rva),
        entry_file_offset: FileOffset::new(file_offset),
        characteristics: 0x1234_5678,
        time_date_stamp: 0x90ab_cdef,
        major_version: 0x1234,
        minor_version: 0xabcd,
        debug_type: 0xfeed_beef,
        size_of_data: u32::MAX,
        address_of_raw_data: RelativeVirtualAddress::new(0xffff_fffd),
        pointer_to_raw_data: FileOffset::new(0xffff_fffe),
    }
}

#[test]
fn both_widths_keep_all_fields_order_zero_rows_and_unfollowed_payloads() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        directory(&mut bytes, plus, 0x1000, 84);
        record(&mut bytes, 512);
        record(&mut bytes, 568);
        let before = bytes.clone();
        let zero = PeDebugDirectoryEntry {
            entry_rva: RelativeVirtualAddress::new(0x101c),
            entry_file_offset: FileOffset::new(540),
            characteristics: 0,
            time_date_stamp: 0,
            major_version: 0,
            minor_version: 0,
            debug_type: 0,
            size_of_data: 0,
            address_of_raw_data: RelativeVirtualAddress::new(0),
            pointer_to_raw_data: FileOffset::new(0),
        };
        assert_eq!(
            parse_pe_debug_directory(&bytes),
            Ok(Some(PeDebugDirectoryTable {
                kind: if plus { PeKind::Pe32Plus } else { PeKind::Pe32 },
                directory_rva: RelativeVirtualAddress::new(0x1000),
                directory_file_offset: FileOffset::new(512),
                directory_size: 84,
                entries: vec![expected(0x1000, 512), zero, expected(0x1038, 568)],
            }))
        );
        assert_eq!(bytes, before);
    }
}

#[test]
fn scalar_metadata_outlives_input_and_unaligned_header_records_are_supported() {
    for plus in [false, true] {
        let table = {
            let mut bytes = fixture(plus);
            directory(&mut bytes, plus, 449, 28);
            record(&mut bytes, 449);
            parse_pe_debug_directory(&bytes).unwrap().unwrap()
        };
        assert_eq!(table.directory_rva, RelativeVirtualAddress::new(449));
        assert_eq!(table.directory_file_offset, FileOffset::new(449));
        assert_eq!(table.entries, [expected(449, 449)]);
    }
}

#[test]
fn missing_and_zero_slots_are_absent() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        directory(&mut bytes, plus, 0, 0);
        assert_eq!(parse_pe_debug_directory(&bytes), Ok(None));
        directory(&mut bytes, plus, u32::MAX, u32::MAX);
        put32(&mut bytes, 152 + fixed(plus) - 4, 6);
        assert_eq!(parse_pe_debug_directory(&bytes), Ok(None));
    }
}

#[test]
fn base_failure_precedes_absence_and_directory_errors() {
    for plus in [false, true] {
        for (rva, size) in [(0, 0), (0, 1), (1, 0), (u32::MAX, 29), (0x1000, 28)] {
            let mut bytes = fixture(plus);
            directory(&mut bytes, plus, rva, size);
            bytes[0] = 0;
            assert_eq!(
                parse_pe_debug_directory(&bytes),
                Err(PeDebugDirectoryError::Base(PeRvaError::Parse(
                    PeHeaderError::InvalidDosSignature {
                        offset: FileOffset::new(0)
                    }
                )))
            );
        }
        let mut bytes = fixture(plus);
        directory(&mut bytes, plus, 0, 0);
        put32(&mut bytes, 212, 1025);
        assert!(matches!(
            parse_pe_debug_directory(&bytes),
            Err(PeDebugDirectoryError::Base(
                PeRvaError::InvalidHeaderExtent { .. }
            ))
        ));
    }
}

#[test]
fn consistency_coordinate_size_and_count_checks_precede_resolution() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        for (rva, size) in [(0, u32::MAX), (u32::MAX, 0)] {
            directory(&mut bytes, plus, rva, size);
            assert_eq!(
                parse_pe_debug_directory(&bytes),
                Err(PeDebugDirectoryError::InconsistentDirectory {
                    rva: RelativeVirtualAddress::new(rva),
                    size,
                })
            );
        }
        directory(&mut bytes, plus, u32::MAX, 7197);
        assert_eq!(
            parse_pe_debug_directory(&bytes),
            Err(PeDebugDirectoryError::DirectoryRangeOverflow {
                rva: RelativeVirtualAddress::new(u32::MAX),
                size: 7197,
            })
        );
        for size in [1, 27, 29, 7197] {
            directory(&mut bytes, plus, 0x2000, size);
            assert_eq!(
                parse_pe_debug_directory(&bytes),
                Err(PeDebugDirectoryError::InvalidDirectorySize { size })
            );
        }
        directory(&mut bytes, plus, 0x2000, 7196);
        assert_eq!(
            parse_pe_debug_directory(&bytes),
            Err(PeDebugDirectoryError::EntryLimitExceeded {
                count: 257,
                limit: 256
            })
        );
    }
}

#[test]
fn exact_256_rows_succeed_but_257_fail_before_reading() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        bytes.resize(512 + 7196, 0);
        section(&mut bytes, plus, 0x1000, 7196);
        directory(&mut bytes, plus, 0x1000, 7168);
        record(&mut bytes, 7652);
        let table = parse_pe_debug_directory(&bytes).unwrap().unwrap();
        assert_eq!(table.entries.len(), 256);
        assert_eq!(table.entries[255], expected(0x2be4, 7652));
        directory(&mut bytes, plus, 0x1000, 7196);
        assert_eq!(
            parse_pe_debug_directory(&bytes),
            Err(PeDebugDirectoryError::EntryLimitExceeded {
                count: 257,
                limit: 256
            })
        );
    }
}

#[test]
fn whole_table_must_be_physically_backed_in_one_region() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        section(&mut bytes, plus, 0x1000, 56);
        directory(&mut bytes, plus, 0x1000, 56);
        for length in 512..568 {
            assert_eq!(
                parse_pe_debug_directory(&bytes[..length]),
                Err(PeDebugDirectoryError::Base(PeRvaError::Parse(
                    PeHeaderError::SectionRawDataOutOfBounds {
                        section_index: 0,
                        section_offset: FileOffset::new(
                            u64::try_from(152 + fixed(plus) + 56).unwrap()
                        ),
                        offset: FileOffset::new(512),
                        needed: 56,
                        available: u64::try_from(length - 512).unwrap(),
                    },
                )))
            );
        }
        assert_eq!(
            parse_pe_debug_directory(&bytes[..568])
                .unwrap()
                .unwrap()
                .entries
                .len(),
            2
        );
        for (rva, cause) in [
            (
                0x3000,
                PeRvaError::UnmappedRva {
                    start: RelativeVirtualAddress::new(0x3000),
                    length: 56,
                },
            ),
            (
                0x101c,
                PeRvaError::CrossesRegionBoundary {
                    start: RelativeVirtualAddress::new(0x101c),
                    length: 56,
                },
            ),
        ] {
            directory(&mut bytes, plus, rva, 56);
            assert_eq!(
                parse_pe_debug_directory(&bytes),
                Err(PeDebugDirectoryError::DirectoryRange {
                    start: RelativeVirtualAddress::new(rva),
                    length: 56,
                    cause,
                })
            );
        }
        directory(&mut bytes, plus, 0x1000, 56);
        put32(&mut bytes, 152 + fixed(plus) + 56 + 16, 28);
        assert_eq!(
            parse_pe_debug_directory(&bytes),
            Err(PeDebugDirectoryError::DirectoryRange {
                start: RelativeVirtualAddress::new(0x1000),
                length: 56,
                cause: PeRvaError::NotFileBacked {
                    start: RelativeVirtualAddress::new(0x1000),
                    length: 56,
                    section_index: 0,
                },
            })
        );
    }
}

#[test]
fn exact_u32_coordinate_end_is_supported_without_wrapping_entry_start() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        bytes.truncate(768);
        section(&mut bytes, plus, 0xffff_ff00, 256);
        directory(&mut bytes, plus, 0xffff_ffe4, 28);
        record(&mut bytes, 740);
        let table = parse_pe_debug_directory(&bytes).unwrap().unwrap();
        assert_eq!(table.directory_file_offset, FileOffset::new(740));
        assert_eq!(table.entries, [expected(0xffff_ffe4, 740)]);
        directory(&mut bytes, plus, 0xffff_ffe5, 28);
        assert_eq!(
            parse_pe_debug_directory(&bytes),
            Err(PeDebugDirectoryError::DirectoryRangeOverflow {
                rva: RelativeVirtualAddress::new(0xffff_ffe5),
                size: 28,
            })
        );
    }
}

fn generated_fixture_with_synthetic_debug_directory(variable: &str, plus: bool) {
    let path = std::env::var_os(variable).expect("explicit generated fixture path");
    let original = std::fs::read(&path).unwrap();
    assert_eq!(original.len(), 1024);
    assert_eq!(&original[60..64], &120_u32.to_le_bytes());
    let slot = if plus { 304 } else { 288 };
    assert_eq!(&original[slot..slot + 8], &[0; 8]);
    assert_eq!(&original[448..504], &[0; 56]);
    let mut bytes = original.clone();
    put32(&mut bytes, slot, 448);
    put32(&mut bytes, slot + 4, 56);
    record(&mut bytes, 448);
    record(&mut bytes, 476);
    let before = bytes.clone();
    assert_eq!(
        parse_pe_debug_directory(&bytes),
        Ok(Some(PeDebugDirectoryTable {
            kind: if plus { PeKind::Pe32Plus } else { PeKind::Pe32 },
            directory_rva: RelativeVirtualAddress::new(448),
            directory_file_offset: FileOffset::new(448),
            directory_size: 56,
            entries: vec![expected(448, 448), expected(476, 476)],
        }))
    );
    assert_eq!(bytes, before);
    assert_eq!(std::fs::read(path).unwrap(), original);
}

#[test]
#[ignore = "requires an explicit generated fixture path"]
fn generated_pe32_with_synthetic_debug_directory_matches_raw_metadata() {
    generated_fixture_with_synthetic_debug_directory("RING3_PE32_FIXTURE", false);
}

#[test]
#[ignore = "requires an explicit generated fixture path"]
fn generated_pe32plus_with_synthetic_debug_directory_matches_raw_metadata() {
    generated_fixture_with_synthetic_debug_directory("RING3_PE32PLUS_FIXTURE", true);
}
