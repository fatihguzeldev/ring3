use ring3_core::{
    FileOffset, PeHeaderError, PeResourceRootError, PeResourceRootName, PeResourceRootNameError,
    PeRvaError, RelativeVirtualAddress, parse_pe_resource_root, parse_pe_resource_root_names,
};

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn put16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
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

fn fixture(plus: bool) -> Vec<u8> {
    let mut bytes = vec![0; 8192];
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 60, 128);
    bytes[128..132].copy_from_slice(b"PE\0\0");
    put16(&mut bytes, 134, 1);
    put16(&mut bytes, 148, u16::try_from(fixed(plus) + 24).unwrap());
    put16(&mut bytes, 152, if plus { 0x20b } else { 0x10b });
    put32(&mut bytes, 212, 512);
    put32(&mut bytes, 152 + fixed(plus) - 4, 3);
    section(&mut bytes, plus, 0x1000, 7680, 7680);
    directory(&mut bytes, plus, 0x1000, 4096);
    put16(&mut bytes, 524, 3);
    put16(&mut bytes, 526, 1);
    for (offset, value) in [
        (528, 0x8000_0040),
        (536, 0x8000_0050),
        (544, 0x8000_0040),
        (552, u32::MAX),
    ] {
        put32(&mut bytes, offset, value);
        put32(&mut bytes, offset + 4, u32::MAX);
    }
    put16(&mut bytes, 576, 4);
    bytes[578..586].copy_from_slice(&[65, 0, 0, 0, 0, 0xd8, 0x3d, 0xd8]);
    bytes
}

fn name(
    index: u16,
    offset: u32,
    rva: u32,
    file: u64,
    count: u16,
    bytes: &[u8],
) -> PeResourceRootName<'_> {
    PeResourceRootName {
        entry_index: index,
        name_offset: offset,
        name_rva: RelativeVirtualAddress::new(rva),
        name_file_offset: FileOffset::new(file),
        code_unit_count: count,
        utf16le: bytes,
    }
}

#[test]
fn both_widths_borrow_exact_ordered_empty_null_and_surrogate_bytes() {
    for plus in [false, true] {
        let bytes = fixture(plus);
        let before = bytes.clone();
        let table = parse_pe_resource_root_names(&bytes).unwrap().unwrap();
        assert_eq!(table.root, parse_pe_resource_root(&bytes).unwrap().unwrap());
        assert_eq!(
            table.names,
            [
                name(0, 64, 0x1040, 576, 4, &[65, 0, 0, 0, 0, 0xd8, 0x3d, 0xd8]),
                name(1, 80, 0x1050, 592, 0, &[]),
                name(2, 64, 0x1040, 576, 4, &[65, 0, 0, 0, 0, 0xd8, 0x3d, 0xd8]),
            ]
        );
        assert_eq!(table.names[0].utf16le.as_ptr(), bytes[578..].as_ptr());
        assert_eq!(table.names[1].utf16le.as_ptr(), bytes[594..].as_ptr());
        assert_eq!(
            table.names[2].utf16le.as_ptr(),
            table.names[0].utf16le.as_ptr()
        );
        assert_eq!(table.root.entries[3].raw_name_or_id, u32::MAX);
        assert_eq!(bytes, before);
    }
}

#[test]
fn absent_roots_and_no_named_entries_do_not_follow_raw_id_words() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        put16(&mut bytes, 524, 0);
        put16(&mut bytes, 526, 4);
        assert!(
            parse_pe_resource_root_names(&bytes)
                .unwrap()
                .unwrap()
                .names
                .is_empty()
        );
        directory(&mut bytes, plus, 0, 0);
        assert_eq!(parse_pe_resource_root_names(&bytes), Ok(None));
        directory(&mut bytes, plus, u32::MAX, u32::MAX);
        put32(&mut bytes, 152 + fixed(plus) - 4, 2);
        assert_eq!(parse_pe_resource_root_names(&bytes), Ok(None));
    }
}

#[test]
fn complete_root_errors_precede_any_named_encoding_or_target() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        bytes[0] = 0;
        assert_eq!(
            parse_pe_resource_root_names(&bytes),
            Err(PeResourceRootNameError::Root(PeResourceRootError::Base(
                PeRvaError::Parse(PeHeaderError::InvalidDosSignature {
                    offset: FileOffset::new(0)
                })
            )))
        );
        bytes[0] = b'M';
        put32(&mut bytes, 528, 0);
        put16(&mut bytes, 524, 257);
        put16(&mut bytes, 526, 0);
        assert_eq!(
            parse_pe_resource_root_names(&bytes),
            Err(PeResourceRootNameError::Root(
                PeResourceRootError::EntryLimitExceeded {
                    count: 257,
                    limit: 256
                }
            ))
        );
        put16(&mut bytes, 524, 3);
        directory(&mut bytes, plus, 0x1000, 16);
        assert_eq!(
            parse_pe_resource_root_names(&bytes),
            Err(PeResourceRootNameError::Root(
                PeResourceRootError::TruncatedRootEntries {
                    size: 16,
                    required: 40
                }
            ))
        );
    }
}

#[test]
fn named_prefix_requires_string_encoding_before_relative_extent() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        for raw in [0, 0x7fff_ffff] {
            put32(&mut bytes, 536, raw);
            assert_eq!(
                parse_pe_resource_root_names(&bytes),
                Err(PeResourceRootNameError::UnsupportedNameEncoding {
                    entry_index: 1,
                    raw_name_or_id: raw
                })
            );
        }
    }
}

#[test]
fn relative_prefix_extent_precedes_coordinate_construction_and_backing() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        for offset in [4095, 4096, 0x7fff_ffff] {
            put32(&mut bytes, 528, 0x8000_0000 | offset);
            assert_eq!(
                parse_pe_resource_root_names(&bytes),
                Err(PeResourceRootNameError::NameOutsideDirectory {
                    entry_index: 0,
                    offset,
                    length: 2,
                    directory_size: 4096
                })
            );
        }
        put32(&mut bytes, 528, 0x8000_2000);
        directory(&mut bytes, plus, 0x1000, 0x3000);
        assert_eq!(
            parse_pe_resource_root_names(&bytes),
            Err(PeResourceRootNameError::NameRange {
                entry_index: 0,
                start: RelativeVirtualAddress::new(0x3000),
                length: 2,
                cause: PeRvaError::UnmappedRva {
                    start: RelativeVirtualAddress::new(0x3000),
                    length: 2
                }
            })
        );
    }
}

#[test]
fn length_limit_precedes_full_relative_extent_and_physical_backing() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        directory(&mut bytes, plus, 0x1000, 66);
        section(&mut bytes, plus, 0x1000, 7680, 66);
        for count in [1025, u16::MAX] {
            put16(&mut bytes, 576, count);
            assert_eq!(
                parse_pe_resource_root_names(&bytes),
                Err(PeResourceRootNameError::NameLengthLimitExceeded {
                    entry_index: 0,
                    code_unit_count: count,
                    limit: 1024
                })
            );
        }
        directory(&mut bytes, plus, 0x1000, 4096);
        section(&mut bytes, plus, 0x1000, 7680, 7680);
        put16(&mut bytes, 524, 1);
        put16(&mut bytes, 526, 0);
        put16(&mut bytes, 576, 1024);
        assert_eq!(
            parse_pe_resource_root_names(&bytes).unwrap().unwrap().names[0]
                .utf16le
                .len(),
            2048
        );
    }
}

#[test]
fn alias_occurrences_charge_exact_aggregate_budget_before_full_extent() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        directory(&mut bytes, plus, 0x1000, 6000);
        put16(&mut bytes, 524, 32);
        put16(&mut bytes, 526, 0);
        for index in 0..33 {
            put32(&mut bytes, 528 + index * 8, 0x8000_0c00);
        }
        put16(&mut bytes, 3584, 1024);
        let table = parse_pe_resource_root_names(&bytes).unwrap().unwrap();
        assert_eq!(table.names.len(), 32);
        assert!(
            table
                .names
                .iter()
                .all(|n| n.utf16le.as_ptr() == bytes[3586..].as_ptr())
        );
        put16(&mut bytes, 524, 33);
        put32(&mut bytes, 528 + 32 * 8, 0x8000_176e);
        put16(&mut bytes, 6510, 1);
        assert_eq!(
            parse_pe_resource_root_names(&bytes),
            Err(PeResourceRootNameError::NameCodeUnitBudgetExceeded {
                entry_index: 32,
                used: 32768,
                code_unit_count: 1,
                limit: 32768
            })
        );
    }
}

#[test]
fn full_name_extent_precedes_full_record_backing_and_keeps_prefix_together() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        directory(&mut bytes, plus, 0x1000, 68);
        section(&mut bytes, plus, 0x1000, 7680, 66);
        assert_eq!(
            parse_pe_resource_root_names(&bytes),
            Err(PeResourceRootNameError::NameOutsideDirectory {
                entry_index: 0,
                offset: 64,
                length: 10,
                directory_size: 68
            })
        );
        directory(&mut bytes, plus, 0x1000, 4096);
        assert_eq!(
            parse_pe_resource_root_names(&bytes),
            Err(PeResourceRootNameError::NameRange {
                entry_index: 0,
                start: RelativeVirtualAddress::new(0x1040),
                length: 10,
                cause: PeRvaError::NotFileBacked {
                    start: RelativeVirtualAddress::new(0x1040),
                    length: 10,
                    section_index: 0
                }
            })
        );
        section(&mut bytes, plus, 0x1000, 66, 66);
        assert_eq!(
            parse_pe_resource_root_names(&bytes),
            Err(PeResourceRootNameError::NameRange {
                entry_index: 0,
                start: RelativeVirtualAddress::new(0x1040),
                length: 10,
                cause: PeRvaError::CrossesRegionBoundary {
                    start: RelativeVirtualAddress::new(0x1040),
                    length: 10
                }
            })
        );
    }
}

#[test]
fn unaligned_record_and_zero_length_at_physical_end_need_no_body_read() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        put16(&mut bytes, 524, 1);
        put16(&mut bytes, 526, 0);
        put32(&mut bytes, 528, 0x8000_0041);
        put16(&mut bytes, 577, 0);
        section(&mut bytes, plus, 0x1000, 67, 67);
        bytes.truncate(579);
        directory(&mut bytes, plus, 0x1000, 0xffff_f000);
        let table = parse_pe_resource_root_names(&bytes).unwrap().unwrap();
        assert_eq!(table.names, [name(0, 65, 0x1041, 577, 0, &[])]);
        assert_eq!(table.names[0].utf16le.as_ptr(), bytes[579..].as_ptr());
    }
}

#[test]
fn final_name_record_can_end_at_the_u32_coordinate_boundary() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        section(&mut bytes, plus, 0xffff_ff00, 256, 256);
        bytes.truncate(768);
        directory(&mut bytes, plus, 0xffff_ff00, 256);
        put16(&mut bytes, 524, 1);
        put16(&mut bytes, 526, 0);
        put32(&mut bytes, 528, 0x8000_00fc);
        put16(&mut bytes, 764, 1);
        put16(&mut bytes, 766, 0xd800);
        assert_eq!(
            parse_pe_resource_root_names(&bytes).unwrap().unwrap().names,
            [name(0, 252, 0xffff_fffc, 764, 1, &[0, 0xd8])]
        );
        put16(&mut bytes, 764, 2);
        assert_eq!(
            parse_pe_resource_root_names(&bytes),
            Err(PeResourceRootNameError::NameOutsideDirectory {
                entry_index: 0,
                offset: 252,
                length: 6,
                directory_size: 256
            })
        );
    }
}

fn generated_fixture_with_synthetic_resource_names(variable: &str, plus: bool) {
    use ring3_core::{PeKind, PeResourceRoot, PeResourceRootEntry};

    let path = std::env::var_os(variable).expect("explicit generated fixture path");
    let original = std::fs::read(&path).unwrap();
    assert_eq!(original.len(), 1024);
    assert_eq!(&original[60..64], &120_u32.to_le_bytes());
    let slot = if plus { 272 } else { 256 };
    assert_eq!(&original[slot..slot + 8], &[0; 8]);
    assert_eq!(&original[448..488], &[0; 40]);
    let mut bytes = original.clone();
    put32(&mut bytes, slot, 448);
    put32(&mut bytes, slot + 4, 40);
    put16(&mut bytes, 460, 1);
    put16(&mut bytes, 462, 1);
    put32(&mut bytes, 464, 0x8000_0020);
    put32(&mut bytes, 468, 0x8000_0000);
    put32(&mut bytes, 472, u32::MAX);
    put32(&mut bytes, 476, u32::MAX);
    bytes[480..488].copy_from_slice(&[3, 0, 65, 0, 0, 0, 0, 0xd8]);
    let before = bytes.clone();
    let table = parse_pe_resource_root_names(&bytes).unwrap().unwrap();
    assert_eq!(
        table.root,
        PeResourceRoot {
            kind: if plus { PeKind::Pe32Plus } else { PeKind::Pe32 },
            directory_rva: RelativeVirtualAddress::new(448),
            directory_file_offset: FileOffset::new(448),
            directory_size: 40,
            characteristics: 0,
            time_date_stamp: 0,
            major_version: 0,
            minor_version: 0,
            number_of_named_entries: 1,
            number_of_id_entries: 1,
            entries: vec![
                PeResourceRootEntry {
                    entry_rva: RelativeVirtualAddress::new(464),
                    entry_file_offset: FileOffset::new(464),
                    raw_name_or_id: 0x8000_0020,
                    raw_data_or_subdirectory: 0x8000_0000
                },
                PeResourceRootEntry {
                    entry_rva: RelativeVirtualAddress::new(472),
                    entry_file_offset: FileOffset::new(472),
                    raw_name_or_id: u32::MAX,
                    raw_data_or_subdirectory: u32::MAX
                },
            ],
        }
    );
    assert_eq!(
        table.names,
        [name(0, 32, 480, 480, 3, &[65, 0, 0, 0, 0, 0xd8])]
    );
    assert_eq!(table.names[0].utf16le.as_ptr(), bytes[482..].as_ptr());
    assert_eq!(bytes, before);
    assert_eq!(std::fs::read(path).unwrap(), original);
}

#[test]
#[ignore = "requires an explicit generated fixture path"]
fn generated_pe32_with_synthetic_resource_names_matches_raw_bytes() {
    generated_fixture_with_synthetic_resource_names("RING3_PE32_FIXTURE", false);
}

#[test]
#[ignore = "requires an explicit generated fixture path"]
fn generated_pe32plus_with_synthetic_resource_names_matches_raw_bytes() {
    generated_fixture_with_synthetic_resource_names("RING3_PE32PLUS_FIXTURE", true);
}

fn generated_resource_fixture_matches_linked_names(variable: &str, plus: bool) {
    let path = std::env::var_os(variable).expect("explicit generated resource fixture path");
    let bytes = std::fs::read(&path).unwrap();
    let before = bytes.clone();
    assert_eq!(bytes.len(), 2048);
    let table = parse_pe_resource_root_names(&bytes).unwrap().unwrap();
    assert_eq!(table.root, parse_pe_resource_root(&bytes).unwrap().unwrap());
    assert_eq!(
        table.root.kind,
        if plus {
            ring3_core::PeKind::Pe32Plus
        } else {
            ring3_core::PeKind::Pe32
        }
    );
    assert_eq!(
        table.names,
        [name(0, 96, 8288, 1632, 3, &[0x52, 0, 0x33, 0, 0xa9, 3])]
    );
    assert_eq!(table.names[0].utf16le.as_ptr(), bytes[1634..].as_ptr());
    assert_eq!(bytes, before);
    assert_eq!(std::fs::read(path).unwrap(), before);
}

#[test]
#[ignore = "requires an explicit generated resource fixture path"]
fn generated_pe32_resource_names_match_linked_bytes() {
    generated_resource_fixture_matches_linked_names("RING3_RESOURCE_PE32_FIXTURE", false);
}

#[test]
#[ignore = "requires an explicit generated resource fixture path"]
fn generated_pe32plus_resource_names_match_linked_bytes() {
    generated_resource_fixture_matches_linked_names("RING3_RESOURCE_PE32PLUS_FIXTURE", true);
}
