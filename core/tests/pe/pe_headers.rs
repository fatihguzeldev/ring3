use ring3_core::{
    FileOffset, PeDirectoryAddress, PeHeaderError, PeKind, PeOptionalHeader,
    RelativeVirtualAddress, parse_pe_header_prefix, parse_pe_headers,
};

const OPTIONAL_OFFSET: usize = 0x98;

fn fixture(plus: bool, optional_size: u16, count: u32) -> Vec<u8> {
    let mut bytes = vec![0; OPTIONAL_OFFSET + usize::from(optional_size)];
    bytes[..2].copy_from_slice(b"MZ");
    bytes[0x3c..0x40].copy_from_slice(&0x80_u32.to_le_bytes());
    bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
    bytes[0x94..0x96].copy_from_slice(&optional_size.to_le_bytes());
    let optional = &mut bytes[OPTIONAL_OFFSET..];
    optional[..2].copy_from_slice(&(if plus { 0x20b_u16 } else { 0x10b }).to_le_bytes());
    let count_offset = if plus { 108 } else { 92 };
    if optional.len() >= count_offset + 4 {
        optional[count_offset..count_offset + 4].copy_from_slice(&count.to_le_bytes());
    }
    bytes
}

#[test]
fn exact_fixed_headers_with_no_directories_are_sufficient() {
    for (plus, fixed, kind) in [(false, 96, PeKind::Pe32), (true, 112, PeKind::Pe32Plus)] {
        let headers = parse_pe_headers(&fixture(plus, fixed, 0)).unwrap();
        assert_eq!(headers.prefix.kind, kind);
        assert_eq!(headers.optional.number_of_rva_and_sizes, 0);
        assert_eq!(
            headers.optional.base_of_data,
            if plus {
                None
            } else {
                Some(RelativeVirtualAddress::new(0))
            }
        );
        assert_eq!(headers.directories, [None; 16]);
    }
}

#[test]
fn pe32_plus_preserves_every_wide_address_and_reservation_value() {
    let mut bytes = fixture(true, 112, 0);
    let optional = &mut bytes[OPTIONAL_OFFSET..];
    for (offset, value) in [
        (24, 0x1_4000_0000_u64),
        (72, u64::MAX),
        (80, 0x2_0000_0000),
        (88, 0x3_0000_0000),
        (96, 0x4_0000_0000),
    ] {
        optional[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    let header = parse_pe_headers(&bytes).unwrap().optional;
    assert_eq!(header.image_base, 0x1_4000_0000);
    assert_eq!(header.size_of_stack_reserve, u64::MAX);
    assert_eq!(header.size_of_stack_commit, 0x2_0000_0000);
    assert_eq!(header.size_of_heap_reserve, 0x3_0000_0000);
    assert_eq!(header.size_of_heap_commit, 0x4_0000_0000);
}

#[test]
fn declared_zero_directories_differ_from_absent_and_certificate_uses_file_offset() {
    let mut bytes = fixture(false, 224, 16);
    let certificate = OPTIONAL_OFFSET + 96 + 4 * 8;
    bytes[certificate..certificate + 4].copy_from_slice(&0xfedc_ba98_u32.to_le_bytes());
    bytes[certificate + 4..certificate + 8].copy_from_slice(&17_u32.to_le_bytes());
    let headers = parse_pe_headers(&bytes).unwrap();
    assert!(headers.directories.iter().all(Option::is_some));
    assert_eq!(
        headers.directories[0].unwrap().address,
        PeDirectoryAddress::Rva(RelativeVirtualAddress::new(0))
    );
    assert_eq!(
        headers.directories[4].unwrap().address,
        PeDirectoryAddress::FileOffset(FileOffset::new(0xfedc_ba98))
    );
    assert_eq!(headers.directories[4].unwrap().size, 17);
}

#[test]
fn short_declaration_cannot_borrow_trailing_bytes_and_prefix_still_succeeds() {
    for (plus, fixed) in [(false, 96), (true, 112)] {
        for declared in [2, fixed - 1] {
            let mut bytes = fixture(plus, declared, 0);
            bytes.extend_from_slice(&[0; 256]);
            assert!(parse_pe_header_prefix(&bytes).is_ok());
            assert_eq!(
                parse_pe_headers(&bytes),
                Err(PeHeaderError::OptionalHeaderExtentTooShort {
                    offset: FileOffset::new(OPTIONAL_OFFSET as u64),
                    required: u64::from(fixed),
                    declared,
                })
            );
        }
    }
}

#[test]
fn all_fixed_fields_preserve_distinct_unknown_and_reserved_values() {
    for (plus, fixed) in [(false, 96), (true, 112)] {
        let mut bytes = fixture(plus, fixed, 0);
        let optional = &mut bytes[OPTIONAL_OFFSET..];
        for (index, byte) in optional.iter_mut().enumerate().skip(2) {
            *byte = u8::try_from(index).unwrap();
        }
        let count_offset = usize::from(fixed) - 4;
        optional[count_offset..].fill(0);
        let expected = PeOptionalHeader {
            linker_version: (2, 3),
            size_of_code: 0x0706_0504,
            size_of_initialized_data: 0x0b0a_0908,
            size_of_uninitialized_data: 0x0f0e_0d0c,
            address_of_entry_point: RelativeVirtualAddress::new(0x1312_1110),
            base_of_code: RelativeVirtualAddress::new(0x1716_1514),
            base_of_data: if plus {
                None
            } else {
                Some(RelativeVirtualAddress::new(0x1b1a_1918))
            },
            image_base: if plus {
                0x1f1e_1d1c_1b1a_1918
            } else {
                0x1f1e_1d1c
            },
            section_alignment: 0x2322_2120,
            file_alignment: 0x2726_2524,
            operating_system_version: (0x2928, 0x2b2a),
            image_version: (0x2d2c, 0x2f2e),
            subsystem_version: (0x3130, 0x3332),
            win32_version_value: 0x3736_3534,
            size_of_image: 0x3b3a_3938,
            size_of_headers: 0x3f3e_3d3c,
            checksum: 0x4342_4140,
            subsystem: 0x4544,
            dll_characteristics: 0x4746,
            size_of_stack_reserve: if plus {
                0x4f4e_4d4c_4b4a_4948
            } else {
                0x4b4a_4948
            },
            size_of_stack_commit: if plus {
                0x5756_5554_5352_5150
            } else {
                0x4f4e_4d4c
            },
            size_of_heap_reserve: if plus {
                0x5f5e_5d5c_5b5a_5958
            } else {
                0x5352_5150
            },
            size_of_heap_commit: if plus {
                0x6766_6564_6362_6160
            } else {
                0x5756_5554
            },
            loader_flags: if plus { 0x6b6a_6968 } else { 0x5b5a_5958 },
            number_of_rva_and_sizes: 0,
        };
        assert_eq!(parse_pe_headers(&bytes).unwrap().optional, expected);
    }
}

#[test]
fn declared_directory_extent_is_checked_before_parser_scope_limit() {
    for (plus, fixed) in [(false, 96), (true, 112)] {
        for (count, declared) in [(1, fixed + 7), (17, fixed + 16 * 8), (u32::MAX, u16::MAX)] {
            let mut bytes = fixture(plus, declared, count);
            bytes.extend_from_slice(&[0; 16]);
            assert_eq!(
                parse_pe_headers(&bytes),
                Err(PeHeaderError::OptionalHeaderExtentTooShort {
                    offset: FileOffset::new(OPTIONAL_OFFSET as u64),
                    required: u64::from(fixed) + u64::from(count) * 8,
                    declared,
                })
            );
        }
        assert_eq!(
            parse_pe_headers(&fixture(plus, fixed + 17 * 8, 17)),
            Err(PeHeaderError::DirectoryLimitExceeded {
                offset: FileOffset::new(OPTIONAL_OFFSET as u64 + u64::from(fixed) - 4),
                count: 17,
                limit: 16,
            })
        );
    }
}

#[test]
fn all_declared_directory_slots_preserve_raw_coordinates_and_sizes() {
    for (plus, fixed) in [(false, 96), (true, 112)] {
        let mut bytes = fixture(plus, fixed + 16 * 8, 16);
        for index in 0..16_usize {
            let offset = OPTIONAL_OFFSET + usize::from(fixed) + index * 8;
            let value = u32::MAX - u32::try_from(index).unwrap();
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            bytes[offset + 4..offset + 8].copy_from_slice(&(value - 16).to_le_bytes());
        }
        let headers = parse_pe_headers(&bytes).unwrap();
        for (index, directory) in headers.directories.iter().enumerate() {
            let directory = directory.unwrap();
            let value = u32::MAX - u32::try_from(index).unwrap();
            let expected = if index == 4 {
                PeDirectoryAddress::FileOffset(FileOffset::new(u64::from(value)))
            } else {
                PeDirectoryAddress::Rva(RelativeVirtualAddress::new(value))
            };
            assert_eq!(directory.address, expected);
            assert_eq!(directory.size, value - 16);
        }
    }
}

#[test]
fn strict_file_truncations_fail_and_trailing_bytes_do_not_change_metadata() {
    for (plus, size) in [(false, 224), (true, 240)] {
        let mut bytes = fixture(plus, size, 16);
        for end in 0..bytes.len() {
            assert!(matches!(
                parse_pe_headers(&bytes[..end]),
                Err(PeHeaderError::OutOfBounds { .. })
            ));
        }
        let exact = parse_pe_headers(&bytes).unwrap();
        bytes.extend_from_slice(&[0xaa; 32]);
        assert_eq!(parse_pe_headers(&bytes), Ok(exact));
    }
}

fn check_recorded_fixture(path_variable: &str, plus: bool) {
    let path = std::env::var_os(path_variable).expect("the explicit fixture path is required");
    let bytes = std::fs::read(path).expect("the generated fixture must be readable");
    assert_eq!(bytes.len(), 1024);
    let headers = parse_pe_headers(&bytes).unwrap();
    assert_eq!(headers.prefix.pe_offset, FileOffset::new(120));
    assert_eq!(headers.prefix.machine, if plus { 0x8664 } else { 0x14c });
    assert_eq!(headers.prefix.number_of_sections, 1);
    assert_eq!(
        headers.prefix.characteristics,
        if plus { 0x23 } else { 0x103 }
    );
    assert_eq!(
        headers.prefix.size_of_optional_header,
        if plus { 240 } else { 224 }
    );
    assert_eq!(
        headers.prefix.kind,
        if plus { PeKind::Pe32Plus } else { PeKind::Pe32 }
    );
    assert_eq!(
        headers.optional,
        PeOptionalHeader {
            linker_version: (14, 0),
            size_of_code: 512,
            size_of_initialized_data: 0,
            size_of_uninitialized_data: 0,
            address_of_entry_point: RelativeVirtualAddress::new(0x1000),
            base_of_code: RelativeVirtualAddress::new(0x1000),
            base_of_data: if plus {
                None
            } else {
                Some(RelativeVirtualAddress::new(0))
            },
            image_base: if plus { 0x1_4000_0000 } else { 0x0040_0000 },
            section_alignment: 0x1000,
            file_alignment: 512,
            operating_system_version: (6, 0),
            image_version: (0, 0),
            subsystem_version: (6, 0),
            win32_version_value: 0,
            size_of_image: 0x2000,
            size_of_headers: 512,
            checksum: 0,
            subsystem: 3,
            dll_characteristics: if plus { 0x8120 } else { 0x8100 },
            size_of_stack_reserve: if plus { 0x1_0000_2000 } else { 0x10_0000 },
            size_of_stack_commit: if plus { 0x1_0000_1000 } else { 0x1000 },
            size_of_heap_reserve: if plus { 0x2_0000_3000 } else { 0x10_0000 },
            size_of_heap_commit: if plus { 0x2_0000_1000 } else { 0x1000 },
            loader_flags: 0,
            number_of_rva_and_sizes: 16,
        }
    );
    for (index, directory) in headers.directories.iter().enumerate() {
        let directory = directory.unwrap();
        assert_eq!(directory.size, 0);
        assert_eq!(
            directory.address,
            if index == 4 {
                PeDirectoryAddress::FileOffset(FileOffset::new(0))
            } else {
                PeDirectoryAddress::Rva(RelativeVirtualAddress::new(0))
            }
        );
    }
}

#[test]
#[ignore = "requires the explicit RING3_PE32_FIXTURE path"]
fn generated_pe32_matches_recorded_header_metadata() {
    check_recorded_fixture("RING3_PE32_FIXTURE", false);
}

#[test]
#[ignore = "requires the explicit RING3_PE32PLUS_FIXTURE path"]
fn disposable_pe32_plus_matches_recorded_header_metadata() {
    check_recorded_fixture("RING3_PE32PLUS_FIXTURE", true);
}
