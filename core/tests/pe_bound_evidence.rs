use ring3_core::{
    FileOffset, PeBoundImportError, PeBoundImportEvidence, PeBoundImportEvidenceError as Error,
    PeBoundImportEvidenceLimits as Limits, PeBoundImportNameError,
    PeBoundImportNameLocation as Location, PeOwnedBoundImportName, PeOwnedBoundImportNameTable,
    RelativeVirtualAddress as Rva, inspect_pe_bound_imports, parse_pe_bound_import_descriptors,
};

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn fixed(plus: bool) -> usize {
    if plus { 112 } else { 96 }
}

fn directory(bytes: &mut [u8], plus: bool, rva: u32, size: u32) {
    put32(bytes, 152 + fixed(plus) + 88, rva);
    put32(bytes, 152 + fixed(plus) + 92, size);
}

fn section(bytes: &mut [u8], plus: bool, index: usize, fields: [u32; 4]) {
    let table = 152 + fixed(plus) + 128 + index * 40;
    for (offset, value) in [8, 12, 16, 20].into_iter().zip(fields) {
        put32(bytes, table + offset, value);
    }
}

fn fixture(plus: bool, size: u32) -> Vec<u8> {
    let mut bytes = vec![0; 12800];
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 60, 128);
    bytes[128..132].copy_from_slice(b"PE\0\0");
    bytes[134..136].copy_from_slice(&2_u16.to_le_bytes());
    bytes[148..150].copy_from_slice(&u16::try_from(fixed(plus) + 128).unwrap().to_le_bytes());
    bytes[152..154].copy_from_slice(&if plus { 0x20b_u16 } else { 0x10b }.to_le_bytes());
    put32(&mut bytes, 212, 512);
    put32(&mut bytes, 152 + fixed(plus) - 4, 16);
    directory(&mut bytes, plus, 4096, size);
    section(&mut bytes, plus, 0, [12288, 4096, 12288, 512]);
    section(&mut bytes, plus, 1, [0, 0x9000, 0, 12800]);
    bytes
}

fn record(bytes: &mut [u8], offset: usize, stamp: u32, name: u16, last: u16) {
    put32(bytes, offset, stamp);
    bytes[offset + 4..offset + 6].copy_from_slice(&name.to_le_bytes());
    bytes[offset + 6..offset + 8].copy_from_slice(&last.to_le_bytes());
}

fn limits(length: usize, rows: u64, text: u64) -> Limits {
    Limits {
        max_input_bytes: length as u64,
        max_output_rows: rows,
        max_output_text_bytes: text,
    }
}

fn name(bytes: &mut [u8], offset: u16, text: &[u8]) {
    let start = 512 + usize::from(offset);
    bytes[start..start + text.len()].copy_from_slice(text);
    bytes[start + text.len()] = 0;
}

fn interleaved(plus: bool) -> Vec<u8> {
    let mut bytes = fixture(plus, 48);
    record(&mut bytes, 512, u32::MAX, 64, 2);
    record(&mut bytes, 520, 0, 80, u16::MAX);
    record(&mut bytes, 528, 1, 64, 7);
    record(&mut bytes, 536, 0, 96, 1);
    record(&mut bytes, 544, 2, 64, 0);
    name(&mut bytes, 64, b"alpha.dll");
    name(&mut bytes, 80, b"B.dll");
    name(&mut bytes, 96, b"c");
    bytes
}

fn owned_name(location: Location, offset: u16, text: &str) -> PeOwnedBoundImportName {
    PeOwnedBoundImportName {
        location,
        name_rva: Rva::new(4096 + u32::from(offset)),
        name_file_offset: FileOffset::new(512 + u64::from(offset)),
        dll_name: text.to_owned(),
    }
}

#[test]
fn both_owned_views_keep_complete_records_and_order_after_input_release() {
    for plus in [false, true] {
        let mut bytes = interleaved(plus);
        let before = bytes.clone();
        let raw = parse_pe_bound_import_descriptors(&bytes).unwrap().unwrap();
        let output = inspect_pe_bound_imports(&bytes, limits(bytes.len(), 15, 33));
        assert_eq!(
            output,
            inspect_pe_bound_imports(&bytes, limits(bytes.len(), 15, 33))
        );
        assert_eq!(bytes, before);
        bytes.fill(0);
        drop(bytes);
        assert_eq!(
            output,
            Ok(PeBoundImportEvidence {
                total_rows: 15,
                total_text_bytes: 33,
                descriptors: Ok(Some(raw.clone())),
                names: Ok(Some(PeOwnedBoundImportNameTable {
                    table: raw,
                    names: vec![
                        owned_name(
                            Location::Descriptor {
                                descriptor_index: 0
                            },
                            64,
                            "alpha.dll"
                        ),
                        owned_name(
                            Location::Forwarder {
                                descriptor_index: 0,
                                forwarder_index: 0
                            },
                            80,
                            "B.dll"
                        ),
                        owned_name(
                            Location::Forwarder {
                                descriptor_index: 0,
                                forwarder_index: 1
                            },
                            64,
                            "alpha.dll"
                        ),
                        owned_name(
                            Location::Descriptor {
                                descriptor_index: 1
                            },
                            96,
                            "c"
                        ),
                        owned_name(
                            Location::Forwarder {
                                descriptor_index: 1,
                                forwarder_index: 0
                            },
                            64,
                            "alpha.dll"
                        ),
                    ]
                })),
            })
        );
    }
}

#[test]
fn complete_input_then_rows_then_text_refusal_keep_actual_operands() {
    for plus in [false, true] {
        let mut bytes = interleaved(plus);
        let length = bytes.len() as u64;
        assert_eq!(
            inspect_pe_bound_imports(
                &bytes,
                Limits {
                    max_input_bytes: length - 1,
                    max_output_rows: 0,
                    max_output_text_bytes: 0
                }
            ),
            Err(Error::InputTooLarge {
                length,
                limit: length - 1
            })
        );
        for rows in [0, 14] {
            assert_eq!(
                inspect_pe_bound_imports(&bytes, limits(bytes.len(), rows, 0)),
                Err(Error::OutputRowsExceeded {
                    rows: 15,
                    limit: rows
                })
            );
        }
        for text in [0, 32] {
            assert_eq!(
                inspect_pe_bound_imports(&bytes, limits(bytes.len(), 15, text)),
                Err(Error::OutputTextExceeded {
                    bytes: 33,
                    limit: text
                })
            );
        }
        assert!(inspect_pe_bound_imports(&bytes, limits(bytes.len(), 15, 33)).is_ok());
        assert!(inspect_pe_bound_imports(&bytes, limits(bytes.len(), u64::MAX, u64::MAX)).is_ok());
        bytes.extend_from_slice(b"unread tail");
        assert_eq!(
            inspect_pe_bound_imports(
                &bytes,
                Limits {
                    max_input_bytes: length,
                    max_output_rows: 15,
                    max_output_text_bytes: 33
                }
            ),
            Err(Error::InputTooLarge {
                length: bytes.len() as u64,
                limit: length
            })
        );
        bytes[0] = 0;
        assert_eq!(
            inspect_pe_bound_imports(&bytes, limits(0, 0, 0)),
            Err(Error::InputTooLarge {
                length: bytes.len() as u64,
                limit: 0
            })
        );
    }
}

#[test]
fn absence_empty_and_raw_errors_keep_distinct_zero_cost_results() {
    let empty_input = inspect_pe_bound_imports(&[], limits(0, 0, 0)).unwrap();
    assert_eq!(
        (empty_input.total_rows, empty_input.total_text_bytes),
        (0, 0)
    );
    assert!(matches!(
        empty_input.descriptors,
        Err(PeBoundImportError::Base(_))
    ));
    assert!(matches!(
        empty_input.names,
        Err(PeBoundImportNameError::Table(PeBoundImportError::Base(_)))
    ));
    for plus in [false, true] {
        let mut bytes = fixture(plus, 8192);
        let output = inspect_pe_bound_imports(&bytes, limits(bytes.len(), 0, 0)).unwrap();
        assert_eq!((output.total_rows, output.total_text_bytes), (0, 0));
        let raw = output.descriptors.unwrap().unwrap();
        assert!(raw.descriptors.is_empty());
        assert_eq!(raw.directory_size, 8192);
        assert_eq!(
            output.names,
            Ok(Some(PeOwnedBoundImportNameTable {
                table: raw,
                names: vec![]
            }))
        );
        directory(&mut bytes, plus, 0, 0);
        let absent = PeBoundImportEvidence {
            total_rows: 0,
            total_text_bytes: 0,
            descriptors: Ok(None),
            names: Ok(None),
        };
        assert_eq!(
            inspect_pe_bound_imports(&bytes, limits(bytes.len(), 0, 0)),
            Ok(absent.clone())
        );
        directory(&mut bytes, plus, u32::MAX, u32::MAX);
        put32(&mut bytes, 152 + fixed(plus) - 4, 11);
        assert_eq!(
            inspect_pe_bound_imports(&bytes, limits(bytes.len(), 0, 0)),
            Ok(absent)
        );
    }
}

#[test]
fn name_failure_preserves_raw_metadata_and_discards_all_name_output_cost() {
    for plus in [false, true] {
        let mut bytes = interleaved(plus);
        name(&mut bytes, 96, b"ab\xff");
        let raw = parse_pe_bound_import_descriptors(&bytes);
        let expected = PeBoundImportNameError::NonAsciiDllName {
            location: Location::Descriptor {
                descriptor_index: 1,
            },
            name_rva: Rva::new(4192),
            offset: 2,
            byte: 255,
        };
        assert_eq!(
            inspect_pe_bound_imports(&bytes, limits(bytes.len(), 4, 0)),
            Err(Error::OutputRowsExceeded { rows: 5, limit: 4 })
        );
        let output = inspect_pe_bound_imports(&bytes, limits(bytes.len(), 5, 0));
        bytes.fill(0);
        drop(bytes);
        assert_eq!(
            output,
            Ok(PeBoundImportEvidence {
                total_rows: 5,
                total_text_bytes: 0,
                descriptors: raw,
                names: Err(expected)
            })
        );
    }
}

#[test]
fn late_raw_failure_disposes_prefix_and_precedes_an_invalid_name() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 8);
        record(&mut bytes, 512, 1, u16::MAX, 0);
        let error = PeBoundImportError::MissingTerminator {
            descriptor_index: 1,
        };
        assert_eq!(
            inspect_pe_bound_imports(&bytes, limits(bytes.len(), 0, 0)),
            Ok(PeBoundImportEvidence {
                total_rows: 0,
                total_text_bytes: 0,
                descriptors: Err(error),
                names: Err(PeBoundImportNameError::Table(error)),
            })
        );
    }
}

#[test]
fn zero_references_overlapping_strings_and_ascii_bytes_preserve_copy_policy() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 24);
        record(&mut bytes, 512, 65, 0, 1);
        let output = inspect_pe_bound_imports(&bytes, limits(bytes.len(), 6, 2)).unwrap();
        assert_eq!((output.total_rows, output.total_text_bytes), (6, 2));
        assert_eq!(
            output
                .names
                .unwrap()
                .unwrap()
                .names
                .iter()
                .map(|n| n.dll_name.as_str())
                .collect::<Vec<_>>(),
            ["A", "A"]
        );
        let mut bytes = fixture(plus, 24);
        record(&mut bytes, 512, 1, 64, 0);
        record(&mut bytes, 520, 2, 65, 0);
        name(&mut bytes, 64, b"A/\\\x01\x7f");
        let output = inspect_pe_bound_imports(&bytes, limits(bytes.len(), 6, 9)).unwrap();
        assert_eq!((output.total_rows, output.total_text_bytes), (6, 9));
        assert_eq!(
            output
                .names
                .unwrap()
                .unwrap()
                .names
                .iter()
                .map(|n| n.dll_name.as_str())
                .collect::<Vec<_>>(),
            ["A/\\\x01\x7f", "/\\\x01\x7f"]
        );
    }
}

#[test]
fn maximum_record_graph_counts_both_tables_and_every_name_occurrence() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 9224);
        record(&mut bytes, 512, 1, 10000, 1024);
        for i in 0..1024 {
            record(&mut bytes, 520 + i * 8, 1, 10000, 0);
        }
        for i in 1..128 {
            record(&mut bytes, 512 + (1024 + i) * 8, 1, 10000, 0);
        }
        name(&mut bytes, 10000, b"x");
        let output = inspect_pe_bound_imports(&bytes, limits(bytes.len(), 3456, 1152)).unwrap();
        assert_eq!((output.total_rows, output.total_text_bytes), (3456, 1152));
        assert_eq!(output.names.unwrap().unwrap().names.len(), 1152);
        assert_eq!(
            inspect_pe_bound_imports(&bytes, limits(bytes.len(), 3455, 0)),
            Err(Error::OutputRowsExceeded {
                rows: 3456,
                limit: 3455
            })
        );
        assert_eq!(
            inspect_pe_bound_imports(&bytes, limits(bytes.len(), 3456, 1151)),
            Err(Error::OutputTextExceeded {
                bytes: 1152,
                limit: 1151
            })
        );
    }
}

#[test]
fn copied_text_excludes_nuls_and_reader_scan_refusal_keeps_raw_rows_only() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 65 * 8);
        for i in 0..64 {
            record(&mut bytes, 512 + i * 8, 1, 2048, 0);
        }
        name(&mut bytes, 2048, &vec![b'A'; 1023]);
        let output = inspect_pe_bound_imports(&bytes, limits(bytes.len(), 192, 65472)).unwrap();
        assert_eq!((output.total_rows, output.total_text_bytes), (192, 65472));
        directory(&mut bytes, plus, 4096, 66 * 8);
        record(&mut bytes, 512 + 64 * 8, 1, 2048, 0);
        let output = inspect_pe_bound_imports(&bytes, limits(bytes.len(), 65, 0)).unwrap();
        assert_eq!((output.total_rows, output.total_text_bytes), (65, 0));
        assert_eq!(output.descriptors.unwrap().unwrap().descriptors.len(), 65);
        assert_eq!(
            output.names,
            Err(PeBoundImportNameError::NameScanBudgetExceeded {
                location: Location::Descriptor {
                    descriptor_index: 64
                },
                name_rva: Rva::new(6144),
                offset: 0,
                limit: 65536,
            })
        );
    }
}
