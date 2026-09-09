use ring3_core::{
    FileOffset, PeHeaderError, PeKind, PeResourceRoot, PeResourceRootEntry, PeResourceRootError,
    PeRvaError, RelativeVirtualAddress, parse_pe_resource_root,
};

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn fixed(plus: bool) -> usize {
    if plus { 112 } else { 96 }
}

fn directory(bytes: &mut [u8], plus: bool, rva: u32, size: u32) {
    put32(bytes, 152 + fixed(plus) + 16, rva);
    put32(bytes, 152 + fixed(plus) + 20, size);
}

fn section(bytes: &mut [u8], plus: bool, rva: u32, virtual_size: u32, raw_size: u32) {
    for (field, value) in [8, 12, 16, 20]
        .into_iter()
        .zip([virtual_size, rva, raw_size, 512])
    {
        put32(bytes, 152 + fixed(plus) + 24 + field, value);
    }
}

fn record(bytes: &mut [u8], offset: usize, named: u16, ids: u16) {
    put32(bytes, offset, 0x1234_5678);
    put32(bytes, offset + 4, 0x90ab_cdef);
    bytes[offset + 8..offset + 12].copy_from_slice(&[0x34, 0x12, 0xcd, 0xab]);
    bytes[offset + 12..offset + 14].copy_from_slice(&named.to_le_bytes());
    bytes[offset + 14..offset + 16].copy_from_slice(&ids.to_le_bytes());
}

fn fixture(plus: bool) -> Vec<u8> {
    let mut bytes = vec![0; 4096];
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 60, 128);
    bytes[128..132].copy_from_slice(b"PE\0\0");
    bytes[134..136].copy_from_slice(&1_u16.to_le_bytes());
    bytes[148..150].copy_from_slice(&u16::try_from(fixed(plus) + 24).unwrap().to_le_bytes());
    bytes[152..154].copy_from_slice(&if plus { 0x20b_u16 } else { 0x10b }.to_le_bytes());
    put32(&mut bytes, 212, 512);
    put32(&mut bytes, 152 + fixed(plus) - 4, 3);
    section(&mut bytes, plus, 0x1000, 3584, 3584);
    directory(&mut bytes, plus, 0x1000, 40);
    record(&mut bytes, 512, 1, 2);
    for (offset, value) in [
        (528, u32::MAX),
        (532, u32::MAX),
        (536, 7),
        (540, 0x8000_0000),
        (544, 7),
        (548, 0x8000_0000),
    ] {
        put32(&mut bytes, offset, value);
    }
    bytes
}

fn entry(rva: u32, offset: u64, name: u32, target: u32) -> PeResourceRootEntry {
    PeResourceRootEntry {
        entry_rva: RelativeVirtualAddress::new(rva),
        entry_file_offset: FileOffset::new(offset),
        raw_name_or_id: name,
        raw_data_or_subdirectory: target,
    }
}

#[test]
fn both_widths_own_raw_ordered_metadata_without_following_names_or_cycles() {
    for plus in [false, true] {
        let root = {
            let bytes = fixture(plus);
            let before = bytes.clone();
            let result = parse_pe_resource_root(&bytes).unwrap().unwrap();
            assert_eq!(bytes, before);
            result
        };
        assert_eq!(
            root,
            PeResourceRoot {
                kind: if plus { PeKind::Pe32Plus } else { PeKind::Pe32 },
                directory_rva: RelativeVirtualAddress::new(0x1000),
                directory_file_offset: FileOffset::new(512),
                directory_size: 40,
                characteristics: 0x1234_5678,
                time_date_stamp: 0x90ab_cdef,
                major_version: 0x1234,
                minor_version: 0xabcd,
                number_of_named_entries: 1,
                number_of_id_entries: 2,
                entries: vec![
                    entry(0x1010, 528, u32::MAX, u32::MAX),
                    entry(0x1018, 536, 7, 0x8000_0000),
                    entry(0x1020, 544, 7, 0x8000_0000)
                ],
            }
        );
    }
}

#[test]
fn absence_is_distinct_from_present_empty_and_zero_entry_roots() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        bytes[512..552].fill(0);
        assert!(
            parse_pe_resource_root(&bytes)
                .unwrap()
                .unwrap()
                .entries
                .is_empty()
        );
        record(&mut bytes, 512, 2, 0);
        assert_eq!(
            parse_pe_resource_root(&bytes).unwrap().unwrap().entries,
            [entry(0x1010, 528, 0, 0), entry(0x1018, 536, 0, 0)]
        );
        directory(&mut bytes, plus, 0, 0);
        assert_eq!(parse_pe_resource_root(&bytes), Ok(None));
        directory(&mut bytes, plus, u32::MAX, u32::MAX);
        put32(&mut bytes, 152 + fixed(plus) - 4, 2);
        assert_eq!(parse_pe_resource_root(&bytes), Ok(None));
    }
}

#[test]
fn base_consistency_coordinate_and_header_minimum_have_explicit_precedence() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        directory(&mut bytes, plus, 0, 0);
        bytes[0] = 0;
        assert_eq!(
            parse_pe_resource_root(&bytes),
            Err(PeResourceRootError::Base(PeRvaError::Parse(
                PeHeaderError::InvalidDosSignature {
                    offset: FileOffset::new(0)
                }
            )))
        );
        bytes[0] = b'M';
        for (rva, size) in [(0, u32::MAX), (u32::MAX, 0)] {
            directory(&mut bytes, plus, rva, size);
            assert_eq!(
                parse_pe_resource_root(&bytes),
                Err(PeResourceRootError::InconsistentDirectory {
                    rva: RelativeVirtualAddress::new(rva),
                    size
                })
            );
        }
        directory(&mut bytes, plus, u32::MAX, 2);
        assert_eq!(
            parse_pe_resource_root(&bytes),
            Err(PeResourceRootError::DirectoryRangeOverflow {
                rva: RelativeVirtualAddress::new(u32::MAX),
                size: 2
            })
        );
        for size in 1..16 {
            directory(&mut bytes, plus, 0x3000, size);
            assert_eq!(
                parse_pe_resource_root(&bytes),
                Err(PeResourceRootError::TruncatedRootHeader {
                    rva: RelativeVirtualAddress::new(0x3000),
                    size,
                    required: 16
                })
            );
        }
        directory(&mut bytes, plus, 0x3000, 16);
        assert_eq!(
            parse_pe_resource_root(&bytes),
            Err(PeResourceRootError::RootRange {
                start: RelativeVirtualAddress::new(0x3000),
                length: 16,
                cause: PeRvaError::UnmappedRva {
                    start: RelativeVirtualAddress::new(0x3000),
                    length: 16
                },
            })
        );
    }
}

#[test]
fn count_limit_precedes_entry_extent_and_wide_count_sum_never_wraps() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        directory(&mut bytes, plus, 0x1000, 16);
        for (named, ids, count) in [
            (0, 257, 257),
            (128, 129, 257),
            (u16::MAX, u16::MAX, 131_070),
        ] {
            record(&mut bytes, 512, named, ids);
            assert_eq!(
                parse_pe_resource_root(&bytes),
                Err(PeResourceRootError::EntryLimitExceeded { count, limit: 256 })
            );
        }
        record(&mut bytes, 512, 128, 128);
        directory(&mut bytes, plus, 0x1000, 2064);
        let root = parse_pe_resource_root(&bytes).unwrap().unwrap();
        assert_eq!(root.entries.len(), 256);
        assert_eq!(root.entries[255], entry(0x1808, 2568, 0, 0));
    }
}

#[test]
fn declared_entry_extent_precedes_full_prefix_backing() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        section(&mut bytes, plus, 0x1000, 3584, 16);
        for size in 16..40 {
            directory(&mut bytes, plus, 0x1000, size);
            assert_eq!(
                parse_pe_resource_root(&bytes),
                Err(PeResourceRootError::TruncatedRootEntries { size, required: 40 })
            );
        }
        directory(&mut bytes, plus, 0x1000, 40);
        assert_eq!(
            parse_pe_resource_root(&bytes),
            Err(PeResourceRootError::RootRange {
                start: RelativeVirtualAddress::new(0x1000),
                length: 40,
                cause: PeRvaError::NotFileBacked {
                    start: RelativeVirtualAddress::new(0x1000),
                    length: 40,
                    section_index: 0
                },
            })
        );
        section(&mut bytes, plus, 0x1000, 3584, 15);
        assert_eq!(
            parse_pe_resource_root(&bytes),
            Err(PeResourceRootError::RootRange {
                start: RelativeVirtualAddress::new(0x1000),
                length: 16,
                cause: PeRvaError::NotFileBacked {
                    start: RelativeVirtualAddress::new(0x1000),
                    length: 16,
                    section_index: 0
                },
            })
        );
    }
}

#[test]
fn complete_root_prefix_cannot_cross_regions_and_declared_tail_is_unread() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        section(&mut bytes, plus, 0x1000, 32, 32);
        record(&mut bytes, 528, 0, 1);
        directory(&mut bytes, plus, 0x1010, 24);
        assert_eq!(
            parse_pe_resource_root(&bytes),
            Err(PeResourceRootError::RootRange {
                start: RelativeVirtualAddress::new(0x1010),
                length: 24,
                cause: PeRvaError::CrossesRegionBoundary {
                    start: RelativeVirtualAddress::new(0x1010),
                    length: 24
                },
            })
        );
        record(&mut bytes, 512, 0, 0);
        section(&mut bytes, plus, 0x1000, 16, 16);
        bytes.truncate(528);
        directory(&mut bytes, plus, 0x1000, 0xffff_f000);
        let root = parse_pe_resource_root(&bytes).unwrap().unwrap();
        assert_eq!(root.directory_size, 0xffff_f000);
        assert!(root.entries.is_empty());
    }
}

#[test]
fn unaligned_header_roots_keep_raw_words_without_reclassifying_them() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        record(&mut bytes, 449, 1, 1);
        for (offset, value) in [(465, 9), (469, u32::MAX), (473, 0x8000_0000), (477, 0)] {
            put32(&mut bytes, offset, value);
        }
        directory(&mut bytes, plus, 449, 32);
        let root = parse_pe_resource_root(&bytes).unwrap().unwrap();
        assert_eq!(root.directory_file_offset, FileOffset::new(449));
        assert_eq!(
            root.entries,
            [
                entry(465, 465, 9, u32::MAX),
                entry(473, 473, 0x8000_0000, 0)
            ]
        );
    }
}

#[test]
fn final_entry_can_end_at_the_u32_coordinate_boundary() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        bytes.truncate(768);
        section(&mut bytes, plus, 0xffff_ff00, 256, 256);
        record(&mut bytes, 744, 0, 1);
        put32(&mut bytes, 760, u32::MAX);
        put32(&mut bytes, 764, u32::MAX);
        directory(&mut bytes, plus, 0xffff_ffe8, 24);
        assert_eq!(
            parse_pe_resource_root(&bytes).unwrap().unwrap().entries,
            [entry(0xffff_fff8, 760, u32::MAX, u32::MAX)]
        );
        directory(&mut bytes, plus, 0xffff_ffe8, 25);
        assert_eq!(
            parse_pe_resource_root(&bytes),
            Err(PeResourceRootError::DirectoryRangeOverflow {
                rva: RelativeVirtualAddress::new(0xffff_ffe8),
                size: 25
            })
        );
    }
}

fn generated_fixture_with_synthetic_resource_root(variable: &str, plus: bool) {
    let path = std::env::var_os(variable).expect("explicit generated fixture path");
    let original = std::fs::read(&path).unwrap();
    assert_eq!(original.len(), 1024);
    assert_eq!(&original[60..64], &120_u32.to_le_bytes());
    let slot = if plus { 272 } else { 256 };
    assert_eq!(&original[slot..slot + 8], &[0; 8]);
    assert_eq!(&original[448..480], &[0; 32]);
    let mut bytes = original.clone();
    put32(&mut bytes, slot, 448);
    put32(&mut bytes, slot + 4, 32);
    record(&mut bytes, 448, 1, 1);
    for (offset, value) in [
        (464, u32::MAX),
        (468, 0x8000_0000),
        (472, 7),
        (476, u32::MAX),
    ] {
        put32(&mut bytes, offset, value);
    }
    let before = bytes.clone();
    assert_eq!(
        parse_pe_resource_root(&bytes),
        Ok(Some(PeResourceRoot {
            kind: if plus { PeKind::Pe32Plus } else { PeKind::Pe32 },
            directory_rva: RelativeVirtualAddress::new(448),
            directory_file_offset: FileOffset::new(448),
            directory_size: 32,
            characteristics: 0x1234_5678,
            time_date_stamp: 0x90ab_cdef,
            major_version: 0x1234,
            minor_version: 0xabcd,
            number_of_named_entries: 1,
            number_of_id_entries: 1,
            entries: vec![
                entry(464, 464, u32::MAX, 0x8000_0000),
                entry(472, 472, 7, u32::MAX)
            ],
        }))
    );
    assert_eq!(bytes, before);
    assert_eq!(std::fs::read(path).unwrap(), original);
}

#[test]
#[ignore = "requires an explicit generated fixture path"]
fn generated_pe32_with_synthetic_resource_root_matches_raw_metadata() {
    generated_fixture_with_synthetic_resource_root("RING3_PE32_FIXTURE", false);
}

#[test]
#[ignore = "requires an explicit generated fixture path"]
fn generated_pe32plus_with_synthetic_resource_root_matches_raw_metadata() {
    generated_fixture_with_synthetic_resource_root("RING3_PE32PLUS_FIXTURE", true);
}
