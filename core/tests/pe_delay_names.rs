use ring3_core::{
    FileOffset, PeDelayImportDescriptor, PeDelayImportError, PeDelayImportName,
    PeDelayImportNameError, PeDelayImportNameTable, PeHeaderError, PeKind, PeRvaError,
    RelativeVirtualAddress, parse_pe_delay_import_names,
};

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn fixed(plus: bool) -> usize {
    if plus { 112 } else { 96 }
}

fn directory(bytes: &mut [u8], plus: bool, rva: u32, size: u32) {
    put32(bytes, 152 + fixed(plus) + 104, rva);
    put32(bytes, 152 + fixed(plus) + 108, size);
}

fn section(bytes: &mut [u8], plus: bool, index: usize, fields: [u32; 4]) {
    let table = 152 + fixed(plus) + 112 + index * 40;
    for (offset, value) in [8, 12, 16, 20].into_iter().zip(fields) {
        put32(bytes, table + offset, value);
    }
}

fn descriptor(bytes: &mut [u8], index: usize, attributes: u32, name: u32) {
    let offset = 512 + index * 32;
    for (i, value) in [attributes, name, u32::MAX, 2, 3, 4, 5, 6]
        .into_iter()
        .enumerate()
    {
        put32(bytes, offset + i * 4, value);
    }
}

fn fixture(plus: bool) -> Vec<u8> {
    let mut bytes = vec![0; 768];
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 60, 128);
    bytes[128..132].copy_from_slice(b"PE\0\0");
    bytes[134..136].copy_from_slice(&2_u16.to_le_bytes());
    let size = u16::try_from(fixed(plus) + 112).unwrap();
    bytes[148..150].copy_from_slice(&size.to_le_bytes());
    bytes[152..154].copy_from_slice(&if plus { 0x20b_u16 } else { 0x10b }.to_le_bytes());
    put32(&mut bytes, 212, 512);
    put32(&mut bytes, 152 + fixed(plus) - 4, 14);
    directory(&mut bytes, plus, 0x1000, 64);
    section(&mut bytes, plus, 0, [128, 0x1000, 128, 512]);
    section(&mut bytes, plus, 1, [128, 0x6000, 128, 640]);
    descriptor(&mut bytes, 0, 1, 0x6000);
    bytes[640..655].copy_from_slice(b"Ring3Delay.dll\0");
    bytes
}

fn raw(index: u32, name: u32) -> PeDelayImportDescriptor {
    PeDelayImportDescriptor {
        descriptor_rva: RelativeVirtualAddress::new(0x1000 + index * 32),
        descriptor_file_offset: FileOffset::new(512 + u64::from(index) * 32),
        attributes: 1,
        dll_name_address: name,
        module_handle_address: u32::MAX,
        import_address_table_address: 2,
        import_name_table_address: 3,
        bound_import_address_table_address: 4,
        unload_import_address_table_address: 5,
        time_date_stamp: 6,
    }
}

fn expected(plus: bool) -> PeDelayImportNameTable<'static> {
    PeDelayImportNameTable {
        kind: if plus { PeKind::Pe32Plus } else { PeKind::Pe32 },
        directory_rva: RelativeVirtualAddress::new(0x1000),
        directory_file_offset: FileOffset::new(512),
        directory_size: 64,
        imports: vec![PeDelayImportName {
            descriptor: raw(0, 0x6000),
            dll_name: "Ring3Delay.dll",
        }],
        terminator_rva: RelativeVirtualAddress::new(0x1020),
        terminator_file_offset: FileOffset::new(544),
    }
}

#[test]
fn both_widths_preserve_exact_borrowed_name_raw_metadata_and_input() {
    for plus in [false, true] {
        let bytes = fixture(plus);
        let before = bytes.clone();
        let table = parse_pe_delay_import_names(&bytes).unwrap().unwrap();
        assert_eq!(table, expected(plus));
        assert_eq!(table.imports[0].dll_name.as_ptr(), bytes[640..].as_ptr());
        assert_eq!(bytes, before);
    }
}

#[test]
fn duplicate_names_retain_distinct_descriptors_and_original_order() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        directory(&mut bytes, plus, 0x1000, 96);
        descriptor(&mut bytes, 1, 1, 0x6000);
        let table = parse_pe_delay_import_names(&bytes).unwrap().unwrap();
        assert_eq!(
            table,
            PeDelayImportNameTable {
                directory_size: 96,
                imports: vec![
                    PeDelayImportName {
                        descriptor: raw(0, 0x6000),
                        dll_name: "Ring3Delay.dll"
                    },
                    PeDelayImportName {
                        descriptor: raw(1, 0x6000),
                        dll_name: "Ring3Delay.dll"
                    },
                ],
                terminator_rva: RelativeVirtualAddress::new(0x1040),
                terminator_file_offset: FileOffset::new(576),
                ..expected(plus)
            }
        );
        assert_eq!(
            table.imports[0].dll_name.as_ptr(),
            table.imports[1].dll_name.as_ptr()
        );
    }
}

#[test]
fn absent_and_present_empty_tables_keep_the_raw_distinction() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        bytes[512..544].fill(0);
        assert_eq!(
            parse_pe_delay_import_names(&bytes),
            Ok(Some(PeDelayImportNameTable {
                imports: Vec::new(),
                terminator_rva: RelativeVirtualAddress::new(0x1000),
                terminator_file_offset: FileOffset::new(512),
                ..expected(plus)
            }))
        );
        directory(&mut bytes, plus, 0, 0);
        assert_eq!(parse_pe_delay_import_names(&bytes), Ok(None));
        directory(&mut bytes, plus, u32::MAX, u32::MAX);
        put32(&mut bytes, 152 + fixed(plus) - 4, 13);
        assert_eq!(parse_pe_delay_import_names(&bytes), Ok(None));
    }
}

#[test]
fn unsupported_attributes_precede_name_targets() {
    for plus in [false, true] {
        for attributes in [0, 2, 3, u32::MAX] {
            let mut bytes = fixture(plus);
            descriptor(&mut bytes, 0, attributes, u32::MAX);
            assert_eq!(
                parse_pe_delay_import_names(&bytes),
                Err(PeDelayImportNameError::UnsupportedAttributes {
                    descriptor_index: 0,
                    attributes,
                })
            );
        }
    }
}

#[test]
fn the_complete_raw_table_is_validated_before_any_name_or_attribute() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        descriptor(&mut bytes, 0, 0, u32::MAX);
        directory(&mut bytes, plus, 0x1000, 32);
        assert_eq!(
            parse_pe_delay_import_names(&bytes),
            Err(PeDelayImportNameError::Table(
                PeDelayImportError::MissingTerminator {
                    descriptor_index: 1
                }
            ))
        );
        directory(&mut bytes, plus, 0, 0);
        bytes[0] = 0;
        assert_eq!(
            parse_pe_delay_import_names(&bytes),
            Err(PeDelayImportNameError::Table(PeDelayImportError::Base(
                PeRvaError::Parse(PeHeaderError::InvalidDosSignature {
                    offset: FileOffset::new(0)
                })
            )))
        );
    }
}

#[test]
fn ascii_case_controls_header_and_zero_rva_are_preserved_without_normalization() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        let name = "MiXeD\u{1}\u{7f}\t.dll";
        bytes[464..464 + name.len()].copy_from_slice(name.as_bytes());
        put32(&mut bytes, 516, 464);
        let table = parse_pe_delay_import_names(&bytes).unwrap().unwrap();
        assert_eq!(table.imports[0].dll_name, name);
        assert_eq!(table.imports[0].dll_name.as_ptr(), bytes[464..].as_ptr());
        put32(&mut bytes, 516, 0);
        let table = parse_pe_delay_import_names(&bytes).unwrap().unwrap();
        assert_eq!(table.imports[0].dll_name, "MZ");
        assert_eq!(table.imports[0].dll_name.as_ptr(), bytes.as_ptr());
        section(&mut bytes, plus, 1, [128, 0x6001, 128, 640]);
        put32(&mut bytes, 516, 0x6001);
        assert_eq!(
            parse_pe_delay_import_names(&bytes)
                .unwrap()
                .unwrap()
                .imports[0]
                .dll_name,
            "Ring3Delay.dll"
        );
    }
}

#[test]
fn empty_and_every_non_ascii_byte_refuse_with_exact_coordinates() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        bytes[640] = 0;
        assert_eq!(
            parse_pe_delay_import_names(&bytes),
            Err(PeDelayImportNameError::EmptyDllName {
                descriptor_index: 0,
                name_rva: RelativeVirtualAddress::new(0x6000),
            })
        );
        for byte in 128..=255 {
            bytes[640] = byte;
            assert_eq!(
                parse_pe_delay_import_names(&bytes),
                Err(PeDelayImportNameError::NonAsciiDllName {
                    descriptor_index: 0,
                    name_rva: RelativeVirtualAddress::new(0x6000),
                    offset: 0,
                    byte,
                })
            );
        }
        bytes[640] = b'R';
        bytes[645] = 255;
        assert_eq!(
            parse_pe_delay_import_names(&bytes),
            Err(PeDelayImportNameError::NonAsciiDllName {
                descriptor_index: 0,
                name_rva: RelativeVirtualAddress::new(0x6000),
                offset: 5,
                byte: 255,
            })
        );
    }
}

#[test]
fn every_consumed_name_prefix_preserves_its_conservative_backing_error() {
    for plus in [false, true] {
        let start = RelativeVirtualAddress::new(0x6000);
        let length = 5;
        for (first, second, cause) in [
            (
                [4, 0x6000, 4, 640],
                [124, 0x6004, 124, 644],
                PeRvaError::CrossesRegionBoundary { start, length },
            ),
            (
                [128, 0x6000, 128, 640],
                [124, 0x6004, 124, 644],
                PeRvaError::AmbiguousRange { start, length },
            ),
            (
                [128, 0x6000, 4, 640],
                [0, 0x8000, 0, 0],
                PeRvaError::NotFileBacked {
                    start,
                    length,
                    section_index: 1,
                },
            ),
            (
                [4, 0x6000, 128, 640],
                [0, 0x8000, 0, 0],
                PeRvaError::RawPaddingUnsupported {
                    start,
                    length,
                    section_index: 1,
                },
            ),
        ] {
            let mut bytes = fixture(plus);
            bytes[134..136].copy_from_slice(&3_u16.to_le_bytes());
            section(&mut bytes, plus, 1, first);
            section(&mut bytes, plus, 2, second);
            assert_eq!(
                parse_pe_delay_import_names(&bytes),
                Err(PeDelayImportNameError::NameRange {
                    descriptor_index: 0,
                    name_rva: start,
                    offset: 4,
                    cause,
                })
            );
        }
        let mut bytes = fixture(plus);
        put32(&mut bytes, 516, u32::MAX);
        assert_eq!(
            parse_pe_delay_import_names(&bytes),
            Err(PeDelayImportNameError::NameRange {
                descriptor_index: 0,
                name_rva: RelativeVirtualAddress::new(u32::MAX),
                offset: 0,
                cause: PeRvaError::UnmappedRva {
                    start: RelativeVirtualAddress::new(u32::MAX),
                    length: 1
                },
            })
        );
    }
}

fn long_names(plus: bool, count: u16, size: u32) -> Vec<u8> {
    let mut bytes = fixture(plus);
    bytes.resize(4640 + size as usize, 0);
    bytes[512..].fill(0);
    section(&mut bytes, plus, 0, [4128, 0x1000, 4128, 512]);
    section(&mut bytes, plus, 1, [size, 0x6000, size, 4640]);
    directory(&mut bytes, plus, 0x1000, (u32::from(count) + 1) * 32);
    for index in 0..usize::from(count) {
        descriptor(&mut bytes, index, 1, 0x6000);
    }
    bytes
}

#[test]
fn per_name_budget_counts_nul_and_refuses_before_reading_the_next_byte() {
    for plus in [false, true] {
        let mut bytes = long_names(plus, 1, 1025);
        bytes[4640..5663].fill(b'x');
        assert_eq!(
            parse_pe_delay_import_names(&bytes)
                .unwrap()
                .unwrap()
                .imports[0]
                .dll_name
                .len(),
            1023
        );
        bytes[5663] = b'x';
        assert_eq!(
            parse_pe_delay_import_names(&bytes),
            Err(PeDelayImportNameError::NameLengthLimitExceeded {
                descriptor_index: 0,
                name_rva: RelativeVirtualAddress::new(0x6000),
                limit: 1024,
            })
        );
    }
}

#[test]
fn total_budget_counts_duplicate_scans_and_attributes_still_precede_it() {
    for plus in [false, true] {
        let mut bytes = long_names(plus, 64, 1024);
        bytes[4640..5663].fill(b'x');
        let table = parse_pe_delay_import_names(&bytes).unwrap().unwrap();
        assert_eq!(table.imports.len(), 64);
        assert!(
            table
                .imports
                .iter()
                .all(|import| import.dll_name.len() == 1023)
        );
        directory(&mut bytes, plus, 0x1000, 66 * 32);
        descriptor(&mut bytes, 64, 1, u32::MAX);
        assert_eq!(
            parse_pe_delay_import_names(&bytes),
            Err(PeDelayImportNameError::NameScanBudgetExceeded {
                descriptor_index: 64,
                name_rva: RelativeVirtualAddress::new(u32::MAX),
                offset: 0,
                limit: 65_536,
            })
        );
        descriptor(&mut bytes, 64, 3, u32::MAX);
        assert_eq!(
            parse_pe_delay_import_names(&bytes),
            Err(PeDelayImportNameError::UnsupportedAttributes {
                descriptor_index: 64,
                attributes: 3,
            })
        );
    }
}

#[test]
fn per_name_limit_wins_when_both_budgets_expire() {
    for plus in [false, true] {
        let mut bytes = long_names(plus, 64, 2049);
        bytes[4640..5663].fill(b'x');
        bytes[5664..6688].fill(b'y');
        descriptor(&mut bytes, 63, 1, 0x6400);
        assert_eq!(
            parse_pe_delay_import_names(&bytes),
            Err(PeDelayImportNameError::NameLengthLimitExceeded {
                descriptor_index: 63,
                name_rva: RelativeVirtualAddress::new(0x6400),
                limit: 1024,
            })
        );
    }
}

fn generated_delay_name_fixture(variable: &str, plus: bool) {
    let path = std::env::var_os(variable).expect("explicit generated delay fixture path");
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(bytes.len(), if plus { 3072 } else { 2560 });
    let (rva, offset, name_rva, name_offset, lookup_rva) = if plus {
        (8200, 1544, 8288, 1632, 8264)
    } else {
        (8192, 1536, 8276, 1620, 8256)
    };
    let table = parse_pe_delay_import_names(&bytes).unwrap().unwrap();
    assert_eq!(
        table,
        PeDelayImportNameTable {
            kind: if plus { PeKind::Pe32Plus } else { PeKind::Pe32 },
            directory_rva: RelativeVirtualAddress::new(rva),
            directory_file_offset: FileOffset::new(offset),
            directory_size: 64,
            imports: vec![PeDelayImportName {
                descriptor: PeDelayImportDescriptor {
                    descriptor_rva: RelativeVirtualAddress::new(rva),
                    descriptor_file_offset: FileOffset::new(offset),
                    attributes: 1,
                    dll_name_address: name_rva,
                    module_handle_address: 12288,
                    import_address_table_address: 12296,
                    import_name_table_address: lookup_rva,
                    bound_import_address_table_address: 0,
                    unload_import_address_table_address: 0,
                    time_date_stamp: 0,
                },
                dll_name: "Ring3Delay.dll",
            }],
            terminator_rva: RelativeVirtualAddress::new(rva + 32),
            terminator_file_offset: FileOffset::new(offset + 32),
        }
    );
    assert_eq!(
        table.imports[0].dll_name.as_ptr(),
        bytes[name_offset..].as_ptr()
    );
    assert_eq!(&bytes[name_offset..name_offset + 15], b"Ring3Delay.dll\0");
}

#[test]
#[ignore = "requires an explicit generated delay fixture path"]
fn generated_pe32_delay_names_match_raw_metadata() {
    generated_delay_name_fixture("RING3_DELAY_PE32_FIXTURE", false);
}

#[test]
#[ignore = "requires an explicit generated delay fixture path"]
fn generated_pe32plus_delay_names_match_raw_metadata() {
    generated_delay_name_fixture("RING3_DELAY_PE32PLUS_FIXTURE", true);
}
