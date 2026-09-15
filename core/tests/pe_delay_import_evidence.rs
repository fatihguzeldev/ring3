use ring3_core::{
    FileOffset, PeDelayImportDescriptor, PeDelayImportError, PeDelayImportEvidence,
    PeDelayImportEvidenceError, PeDelayImportEvidenceLimits, PeDelayImportLookupError,
    PeDelayImportNameError, PeDelayImportTable, PeHeaderError, PeImportLookupError, PeKind,
    PeOwnedDelayImportLookup, PeOwnedDelayImportLookupTable, PeOwnedDelayImportName,
    PeOwnedDelayImportNameTable, PeOwnedImportLookupEntry, PeOwnedImportSymbol, PeRvaError,
    RelativeVirtualAddress, inspect_pe_delay_imports,
};

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn fixture(plus: bool) -> Vec<u8> {
    let mut bytes = vec![0; 4096];
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 60, 128);
    bytes[128..132].copy_from_slice(b"PE\0\0");
    bytes[134..136].copy_from_slice(&1_u16.to_le_bytes());
    let fixed = if plus { 112_u16 } else { 96 };
    bytes[148..150].copy_from_slice(&(fixed + 112).to_le_bytes());
    bytes[152..154].copy_from_slice(&if plus { 0x20b_u16 } else { 0x10b }.to_le_bytes());
    put32(&mut bytes, 212, 512);
    put32(&mut bytes, 152 + usize::from(fixed) - 4, 14);
    put32(&mut bytes, 152 + usize::from(fixed) + 104, 0x1000);
    put32(&mut bytes, 152 + usize::from(fixed) + 108, 96);
    for (offset, value) in [(8, 3584), (12, 0x1000), (16, 3584), (20, 512)] {
        put32(&mut bytes, 152 + usize::from(fixed) + 112 + offset, value);
    }
    for (i, fields) in [
        [
            1,
            0x1400,
            u32::MAX,
            0xdead_beef,
            0x1600,
            0x1234_5678,
            2,
            0x8765_4321,
        ],
        [1, 0x1400, 5, 0x1640, 0x1640, 0, 7, u32::MAX],
    ]
    .into_iter()
    .enumerate()
    {
        for (j, value) in fields.into_iter().enumerate() {
            put32(&mut bytes, 512 + i * 32 + j * 4, value);
        }
    }
    bytes[1536..1546].copy_from_slice(b"Own\t.DLL\x01\0");
    bytes[2560..2562].copy_from_slice(&0xabcd_u16.to_le_bytes());
    bytes[2562..2569].copy_from_slice(b"Name\"\t\0");
    let width = if plus { 8 } else { 4 };
    let flag = if plus { 1_u64 << 63 } else { 1 << 31 };
    for (i, raw) in [0x1800, flag | 65535, 0x1800].into_iter().enumerate() {
        let offset = 2048 + i * width;
        bytes[offset..offset + width].copy_from_slice(&raw.to_le_bytes()[..width]);
    }
    bytes[2112..2112 + width].copy_from_slice(&(flag | 32768).to_le_bytes()[..width]);
    bytes
}

fn limits() -> PeDelayImportEvidenceLimits {
    PeDelayImportEvidenceLimits {
        max_input_bytes: 4096,
        max_output_rows: 10,
        max_output_text_bytes: 48,
    }
}

fn raw(index: u32) -> PeDelayImportDescriptor {
    PeDelayImportDescriptor {
        descriptor_rva: RelativeVirtualAddress::new(0x1000 + index * 32),
        descriptor_file_offset: FileOffset::new(512 + u64::from(index) * 32),
        attributes: 1,
        dll_name_address: 0x1400,
        module_handle_address: if index == 0 { u32::MAX } else { 5 },
        import_address_table_address: if index == 0 { 0xdead_beef } else { 0x1640 },
        import_name_table_address: 0x1600 + index * 64,
        bound_import_address_table_address: if index == 0 { 0x1234_5678 } else { 0 },
        unload_import_address_table_address: if index == 0 { 2 } else { 7 },
        time_date_stamp: if index == 0 { 0x8765_4321 } else { u32::MAX },
    }
}

fn expected(plus: bool, empty: bool) -> PeDelayImportEvidence {
    let descriptors = if empty { vec![] } else { vec![raw(0), raw(1)] };
    let names: Vec<_> = descriptors
        .iter()
        .map(|d| PeOwnedDelayImportName {
            descriptor: *d,
            dll_name: "Own\t.DLL\x01".to_owned(),
        })
        .collect();
    let kind = if plus { PeKind::Pe32Plus } else { PeKind::Pe32 };
    let end = if empty { 0 } else { 64 };
    let directory_rva = RelativeVirtualAddress::new(0x1000);
    let directory_file_offset = FileOffset::new(512);
    let terminator_rva = RelativeVirtualAddress::new(0x1000 + end);
    let terminator_file_offset = FileOffset::new(512 + u64::from(end));
    let width = if plus { 8_u32 } else { 4 };
    let flag = if plus { 1_u64 << 63 } else { 1 << 31 };
    let symbol = PeOwnedImportSymbol::ByName {
        hint_name_rva: RelativeVirtualAddress::new(0x1800),
        hint: 0xabcd,
        name: "Name\"\t".to_owned(),
    };
    let lookups = if empty {
        vec![]
    } else {
        vec![
            PeOwnedDelayImportLookup {
                import: names[0].clone(),
                entries: [symbol.clone(), PeOwnedImportSymbol::Ordinal(65535), symbol]
                    .into_iter()
                    .zip(0_u32..)
                    .map(|(symbol, i)| PeOwnedImportLookupEntry {
                        lookup_rva: RelativeVirtualAddress::new(0x1600 + i * width),
                        lookup_file_offset: FileOffset::new(2048 + u64::from(i * width)),
                        raw_value: if i == 1 { flag | 65535 } else { 0x1800 },
                        symbol,
                    })
                    .collect(),
            },
            PeOwnedDelayImportLookup {
                import: names[1].clone(),
                entries: vec![PeOwnedImportLookupEntry {
                    lookup_rva: RelativeVirtualAddress::new(0x1640),
                    lookup_file_offset: FileOffset::new(2112),
                    raw_value: flag | 32768,
                    symbol: PeOwnedImportSymbol::Ordinal(32768),
                }],
            },
        ]
    };
    PeDelayImportEvidence {
        total_rows: if empty { 0 } else { 10 },
        total_text_bytes: if empty { 0 } else { 48 },
        descriptors: Ok(Some(PeDelayImportTable {
            kind,
            directory_rva,
            directory_file_offset,
            directory_size: 96,
            descriptors,
            terminator_rva,
            terminator_file_offset,
        })),
        names: Ok(Some(PeOwnedDelayImportNameTable {
            kind,
            directory_rva,
            directory_file_offset,
            directory_size: 96,
            imports: names,
            terminator_rva,
            terminator_file_offset,
        })),
        lookups: Ok(Some(PeOwnedDelayImportLookupTable {
            kind,
            directory_rva,
            directory_file_offset,
            directory_size: 96,
            imports: lookups,
            terminator_rva,
            terminator_file_offset,
        })),
    }
}

#[test]
fn both_widths_preserve_all_owned_fields_duplicates_and_input_lifetime() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        let before = bytes.clone();
        let result = inspect_pe_delay_imports(&bytes, limits()).unwrap();
        assert_eq!(result, inspect_pe_delay_imports(&bytes, limits()).unwrap());
        assert_eq!(bytes, before);
        bytes.fill(0xee);
        drop(bytes);
        assert_eq!(result, expected(plus, false));
    }
}

#[test]
fn complete_input_row_and_text_limits_have_exact_operands_and_priority() {
    for plus in [false, true] {
        let bytes = fixture(plus);
        assert!(inspect_pe_delay_imports(&bytes, limits()).is_ok());
        for (caps, error) in [
            (
                [4095, 0, 0],
                PeDelayImportEvidenceError::InputTooLarge {
                    length: 4096,
                    limit: 4095,
                },
            ),
            (
                [4096, 9, 0],
                PeDelayImportEvidenceError::OutputRowsExceeded { rows: 10, limit: 9 },
            ),
            (
                [4096, 10, 47],
                PeDelayImportEvidenceError::OutputTextExceeded {
                    bytes: 48,
                    limit: 47,
                },
            ),
        ] {
            assert_eq!(
                inspect_pe_delay_imports(
                    &bytes,
                    PeDelayImportEvidenceLimits {
                        max_input_bytes: caps[0],
                        max_output_rows: caps[1],
                        max_output_text_bytes: caps[2],
                    }
                ),
                Err(error)
            );
        }
    }
}

#[test]
fn unsupported_attributes_keep_raw_tables_and_precede_earlier_missing_int() {
    for plus in [false, true] {
        for attributes in [0, 2] {
            let mut bytes = fixture(plus);
            put32(&mut bytes, 528, 0);
            put32(&mut bytes, 544, attributes);
            let caps = PeDelayImportEvidenceLimits {
                max_output_rows: 2,
                max_output_text_bytes: 0,
                ..limits()
            };
            let result = inspect_pe_delay_imports(&bytes, caps).unwrap();
            bytes.fill(0);
            drop(bytes);
            let mut table = expected(plus, false).descriptors.unwrap().unwrap();
            table.descriptors[0].import_name_table_address = 0;
            table.descriptors[1].attributes = attributes;
            assert_eq!(result.descriptors, Ok(Some(table)));
            assert_eq!((result.total_rows, result.total_text_bytes), (2, 0));
            let error = PeDelayImportNameError::UnsupportedAttributes {
                descriptor_index: 1,
                attributes,
            };
            assert_eq!(result.names, Err(error));
            assert_eq!(result.lookups, Err(PeDelayImportLookupError::Names(error)));
            assert_eq!(
                inspect_pe_delay_imports(&fixture(plus), caps),
                Err(PeDelayImportEvidenceError::OutputRowsExceeded { rows: 10, limit: 2 })
            );
        }
    }
}

#[test]
fn missing_int_keeps_raw_and_names_without_readable_iat_fallback() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        put32(&mut bytes, 560, 0);
        let caps = PeDelayImportEvidenceLimits {
            max_output_rows: 4,
            max_output_text_bytes: 18,
            ..limits()
        };
        let result = inspect_pe_delay_imports(&bytes, caps).unwrap();
        let mut expected = expected(plus, false);
        expected.total_rows = 4;
        expected.total_text_bytes = 18;
        expected
            .descriptors
            .as_mut()
            .unwrap()
            .as_mut()
            .unwrap()
            .descriptors[1]
            .import_name_table_address = 0;
        expected.names.as_mut().unwrap().as_mut().unwrap().imports[1]
            .descriptor
            .import_name_table_address = 0;
        expected.lookups = Err(PeDelayImportLookupError::Lookup(
            PeImportLookupError::LookupTableUnavailable {
                descriptor_index: 1,
            },
        ));
        assert_eq!(
            inspect_pe_delay_imports(
                &bytes,
                PeDelayImportEvidenceLimits {
                    max_output_text_bytes: 17,
                    ..caps
                }
            ),
            Err(PeDelayImportEvidenceError::OutputTextExceeded {
                bytes: 18,
                limit: 17
            })
        );
        bytes.fill(0);
        drop(bytes);
        assert_eq!(result, expected);
    }
}

#[test]
fn absent_zero_and_present_empty_tables_remain_distinct_at_zero_output_caps() {
    for plus in [false, true] {
        for mode in 0..3 {
            let mut bytes = fixture(plus);
            let fixed = if plus { 112 } else { 96 };
            match mode {
                0 => put32(&mut bytes, 152 + fixed - 4, 13),
                1 => bytes[152 + fixed + 104..152 + fixed + 112].fill(0),
                _ => bytes[512..544].fill(0),
            }
            let caps = PeDelayImportEvidenceLimits {
                max_output_rows: 0,
                max_output_text_bytes: 0,
                ..limits()
            };
            let result = inspect_pe_delay_imports(&bytes, caps).unwrap();
            bytes.fill(0);
            drop(bytes);
            if mode == 2 {
                assert_eq!(result, expected(plus, true));
            } else {
                assert_eq!(
                    result,
                    PeDelayImportEvidence {
                        total_rows: 0,
                        total_text_bytes: 0,
                        descriptors: Ok(None),
                        names: Ok(None),
                        lookups: Ok(None),
                    }
                );
            }
        }
    }
}

#[test]
fn base_errors_cost_zero_and_input_admission_precedes_them() {
    let bytes = vec![0; 4096];
    let caps = PeDelayImportEvidenceLimits {
        max_output_rows: 0,
        max_output_text_bytes: 0,
        ..limits()
    };
    let error = PeDelayImportError::Base(PeRvaError::Parse(PeHeaderError::InvalidDosSignature {
        offset: FileOffset::new(0),
    }));
    assert_eq!(
        inspect_pe_delay_imports(&bytes, caps),
        Ok(PeDelayImportEvidence {
            total_rows: 0,
            total_text_bytes: 0,
            descriptors: Err(error),
            names: Err(PeDelayImportNameError::Table(error)),
            lookups: Err(PeDelayImportLookupError::Names(
                PeDelayImportNameError::Table(error)
            )),
        })
    );
    assert_eq!(
        inspect_pe_delay_imports(
            &bytes,
            PeDelayImportEvidenceLimits {
                max_input_bytes: 4095,
                ..caps
            }
        ),
        Err(PeDelayImportEvidenceError::InputTooLarge {
            length: 4096,
            limit: 4095
        })
    );
}

#[test]
fn name_and_symbol_failures_discard_only_their_dependent_views() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        put32(&mut bytes, 528, 0);
        put32(&mut bytes, 548, 0x1420);
        let result = inspect_pe_delay_imports(
            &bytes,
            PeDelayImportEvidenceLimits {
                max_output_rows: 2,
                max_output_text_bytes: 0,
                ..limits()
            },
        )
        .unwrap();
        let error = PeDelayImportNameError::EmptyDllName {
            descriptor_index: 1,
            name_rva: RelativeVirtualAddress::new(0x1420),
        };
        assert_eq!(result.descriptors.unwrap().unwrap().descriptors.len(), 2);
        assert_eq!(result.names, Err(error));
        assert_eq!(result.lookups, Err(PeDelayImportLookupError::Names(error)));
        let mut bytes = fixture(plus);
        bytes[2562] = 255;
        let result = inspect_pe_delay_imports(
            &bytes,
            PeDelayImportEvidenceLimits {
                max_output_rows: 4,
                max_output_text_bytes: 18,
                ..limits()
            },
        )
        .unwrap();
        assert_eq!((result.total_rows, result.total_text_bytes), (4, 18));
        let expected = expected(plus, false);
        assert_eq!(result.descriptors, expected.descriptors);
        assert_eq!(result.names, expected.names);
        assert_eq!(
            result.lookups,
            Err(PeDelayImportLookupError::Lookup(
                PeImportLookupError::NonAsciiSymbolName {
                    descriptor_index: 0,
                    entry_index: 0,
                    hint_name_rva: RelativeVirtualAddress::new(0x1800),
                    offset: 0,
                    byte: 255,
                }
            ))
        );
    }
}

fn compiled_delay_expected(plus: bool, ordinal: bool) -> PeDelayImportEvidence {
    let (rva, offset, lookup_rva, lookup_offset) = if plus {
        (8200, 1544, 8264, 1608)
    } else {
        (8192, 1536, 8256, 1600)
    };
    let name_rva = match (plus, ordinal) {
        (false, false) => 8276,
        (true, false) => 8288,
        (false, true) => 8268,
        (true, true) => 8280,
    };
    let hint_rva = if plus { 8280 } else { 8268 };
    let descriptor = PeDelayImportDescriptor {
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
    };
    let import = PeOwnedDelayImportName {
        descriptor,
        dll_name: "Ring3Delay.dll".to_owned(),
    };
    let entry = PeOwnedImportLookupEntry {
        lookup_rva: RelativeVirtualAddress::new(lookup_rva),
        lookup_file_offset: FileOffset::new(lookup_offset),
        raw_value: if ordinal {
            if plus {
                0x8000_0000_0000_8000
            } else {
                0x8000_8000
            }
        } else {
            u64::from(hint_rva)
        },
        symbol: if ordinal {
            PeOwnedImportSymbol::Ordinal(32768)
        } else {
            PeOwnedImportSymbol::ByName {
                hint_name_rva: RelativeVirtualAddress::new(hint_rva),
                hint: 0,
                name: "probe".to_owned(),
            }
        },
    };
    let kind = if plus { PeKind::Pe32Plus } else { PeKind::Pe32 };
    let directory_rva = RelativeVirtualAddress::new(rva);
    let directory_file_offset = FileOffset::new(offset);
    let terminator_rva = RelativeVirtualAddress::new(rva + 32);
    let terminator_file_offset = FileOffset::new(offset + 32);
    PeDelayImportEvidence {
        total_rows: 4,
        total_text_bytes: if ordinal { 28 } else { 33 },
        descriptors: Ok(Some(PeDelayImportTable {
            kind,
            directory_rva,
            directory_file_offset,
            directory_size: 64,
            descriptors: vec![descriptor],
            terminator_rva,
            terminator_file_offset,
        })),
        names: Ok(Some(PeOwnedDelayImportNameTable {
            kind,
            directory_rva,
            directory_file_offset,
            directory_size: 64,
            imports: vec![import.clone()],
            terminator_rva,
            terminator_file_offset,
        })),
        lookups: Ok(Some(PeOwnedDelayImportLookupTable {
            kind,
            directory_rva,
            directory_file_offset,
            directory_size: 64,
            imports: vec![PeOwnedDelayImportLookup {
                import,
                entries: vec![entry],
            }],
            terminator_rva,
            terminator_file_offset,
        })),
    }
}

fn compiled_owned_delay_fixtures() -> Vec<(std::path::PathBuf, Vec<u8>, PeDelayImportEvidence)> {
    let inputs = [
        ("RING3_DELAY_PE32_FIXTURE", false, false),
        ("RING3_DELAY_PE32PLUS_FIXTURE", true, false),
        ("RING3_DELAY_ORDINAL_PE32_FIXTURE", false, true),
        ("RING3_DELAY_ORDINAL_PE32PLUS_FIXTURE", true, true),
    ]
    .map(|(variable, plus, ordinal)| {
        let path = std::env::var_os(variable).unwrap_or_else(|| panic!("{variable}: NotPresent"));
        (std::path::PathBuf::from(path), plus, ordinal)
    });
    inputs
        .into_iter()
        .map(|(path, plus, ordinal)| {
            let bytes = std::fs::read(&path).unwrap();
            assert_eq!(bytes.len(), if plus { 3072 } else { 2560 });
            (path, bytes, compiled_delay_expected(plus, ordinal))
        })
        .collect()
}

#[test]
#[ignore = "requires all four explicit named/ordinal delay fixture paths"]
fn generated_owned_delay_imports_preserve_compiled_metadata() {
    for (path, mut bytes, expected) in compiled_owned_delay_fixtures() {
        let before = bytes.clone();
        let caps = PeDelayImportEvidenceLimits {
            max_input_bytes: u64::try_from(bytes.len()).unwrap(),
            max_output_rows: expected.total_rows,
            max_output_text_bytes: expected.total_text_bytes,
        };
        let actual = inspect_pe_delay_imports(&bytes, caps).unwrap();
        assert_eq!(actual, inspect_pe_delay_imports(&bytes, caps).unwrap());
        assert_eq!(bytes, before);
        bytes.fill(0xee);
        drop(bytes);
        assert_eq!(actual, expected);
        assert_eq!(std::fs::read(path).unwrap(), before);
    }
}

#[test]
#[ignore = "requires all four explicit named/ordinal delay fixture paths"]
fn generated_owned_delay_import_refusals_preserve_compiled_budget_operands() {
    for (path, bytes, expected) in compiled_owned_delay_fixtures() {
        let before = bytes.clone();
        let length = u64::try_from(bytes.len()).unwrap();
        let rows = expected.total_rows;
        let text = expected.total_text_bytes;
        let caps = PeDelayImportEvidenceLimits {
            max_input_bytes: length,
            max_output_rows: rows,
            max_output_text_bytes: text,
        };
        assert_eq!(inspect_pe_delay_imports(&bytes, caps), Ok(expected));
        for (limits, error) in [
            (
                PeDelayImportEvidenceLimits {
                    max_input_bytes: length - 1,
                    max_output_rows: 0,
                    max_output_text_bytes: 0,
                },
                PeDelayImportEvidenceError::InputTooLarge {
                    length,
                    limit: length - 1,
                },
            ),
            (
                PeDelayImportEvidenceLimits {
                    max_output_rows: rows - 1,
                    max_output_text_bytes: 0,
                    ..caps
                },
                PeDelayImportEvidenceError::OutputRowsExceeded {
                    rows,
                    limit: rows - 1,
                },
            ),
            (
                PeDelayImportEvidenceLimits {
                    max_output_text_bytes: text - 1,
                    ..caps
                },
                PeDelayImportEvidenceError::OutputTextExceeded {
                    bytes: text,
                    limit: text - 1,
                },
            ),
        ] {
            assert_eq!(inspect_pe_delay_imports(&bytes, limits), Err(error));
        }
        assert_eq!(bytes, before);
        assert_eq!(std::fs::read(path).unwrap(), before);
    }
}
