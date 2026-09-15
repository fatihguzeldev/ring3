use ring3_core::{
    FileOffset, PeHeaderError, PeImportError, PeImportLookupError, PeOwnedImportDescriptor,
    PeOwnedImportSymbol, PeRvaError, PeStaticImportEvidenceError, PeStaticImportEvidenceLimits,
    RelativeVirtualAddress, inspect_pe_static_imports,
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
    bytes[148..150].copy_from_slice(&(fixed + 16).to_le_bytes());
    bytes[152..154].copy_from_slice(&if plus { 0x20b_u16 } else { 0x10b }.to_le_bytes());
    put32(&mut bytes, 212, 512);
    put32(&mut bytes, 152 + usize::from(fixed) - 4, 2);
    put32(&mut bytes, 152 + usize::from(fixed) + 8, 0x1000);
    put32(&mut bytes, 152 + usize::from(fixed) + 12, 60);
    for (offset, value) in [(8, 3584), (12, 0x1000), (16, 3584), (20, 512)] {
        put32(&mut bytes, 152 + usize::from(fixed) + 16 + offset, value);
    }
    for (i, fields) in [
        [0x1600, 0x8765_4321, 0x1234_5678, 0x1400, 0xfedc_ba98],
        [0x1640, 0, u32::MAX, 0x1420, 0],
    ]
    .into_iter()
    .enumerate()
    {
        for (j, value) in fields.into_iter().enumerate() {
            put32(&mut bytes, 512 + i * 20 + j * 4, value);
        }
    }
    bytes[1536..1546].copy_from_slice(b"Own\t.DLL\x01\0");
    bytes[1568..1574].copy_from_slice(b"Other\0");
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

fn limits() -> PeStaticImportEvidenceLimits {
    PeStaticImportEvidenceLimits {
        max_input_bytes: 4096,
        max_output_rows: 8,
        max_output_text_bytes: 40,
    }
}

#[test]
fn both_widths_own_ordered_raw_metadata_and_duplicate_names_after_input_drop() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        let original = bytes.clone();
        let evidence = inspect_pe_static_imports(&bytes, limits()).unwrap();
        assert_eq!(
            evidence,
            inspect_pe_static_imports(&bytes, limits()).unwrap()
        );
        assert_eq!(bytes, original);
        bytes.fill(0xee);
        drop(bytes);
        assert_eq!((evidence.total_rows, evidence.total_text_bytes), (8, 40));
        let descriptors = evidence.descriptors.unwrap();
        assert_eq!(
            descriptors,
            vec![
                PeOwnedImportDescriptor {
                    descriptor_rva: RelativeVirtualAddress::new(0x1000),
                    descriptor_file_offset: FileOffset::new(512),
                    import_lookup_table_rva: RelativeVirtualAddress::new(0x1600),
                    time_date_stamp: 0x8765_4321,
                    forwarder_chain: 0x1234_5678,
                    name_rva: RelativeVirtualAddress::new(0x1400),
                    import_address_table_rva: RelativeVirtualAddress::new(0xfedc_ba98),
                    dll_name: "Own\t.DLL\x01".to_owned(),
                },
                PeOwnedImportDescriptor {
                    descriptor_rva: RelativeVirtualAddress::new(0x1014),
                    descriptor_file_offset: FileOffset::new(532),
                    import_lookup_table_rva: RelativeVirtualAddress::new(0x1640),
                    time_date_stamp: 0,
                    forwarder_chain: u32::MAX,
                    name_rva: RelativeVirtualAddress::new(0x1420),
                    import_address_table_rva: RelativeVirtualAddress::new(0),
                    dll_name: "Other".to_owned(),
                },
            ]
        );
        let lookups = evidence.lookups.unwrap();
        assert_eq!(lookups.len(), 2);
        assert_eq!(lookups[0].descriptor, descriptors[0]);
        assert_eq!(lookups[1].descriptor, descriptors[1]);
        assert_eq!(lookups[0].entries.len(), 3);
        assert_eq!(lookups[1].entries.len(), 1);
        let flag = if plus { 1_u64 << 63 } else { 1 << 31 };
        let width = if plus { 8 } else { 4 };
        for (i, entry) in (0_u32..).zip(&lookups[0].entries) {
            assert_eq!(
                entry.lookup_rva,
                RelativeVirtualAddress::new(0x1600 + i * width)
            );
            assert_eq!(
                entry.lookup_file_offset,
                FileOffset::new(2048 + u64::from(i * width))
            );
            assert_eq!(entry.raw_value, if i == 1 { flag | 65535 } else { 0x1800 });
        }
        let named = PeOwnedImportSymbol::ByName {
            hint_name_rva: RelativeVirtualAddress::new(0x1800),
            hint: 0xabcd,
            name: "Name\"\t".to_owned(),
        };
        assert_eq!(lookups[0].entries[0].symbol, named);
        assert_eq!(lookups[0].entries[2].symbol, named);
        assert_eq!(
            lookups[0].entries[1].symbol,
            PeOwnedImportSymbol::Ordinal(65535)
        );
        assert_eq!(
            lookups[1].entries[0].symbol,
            PeOwnedImportSymbol::Ordinal(32768)
        );
    }
}

#[test]
fn input_then_total_rows_then_total_text_refuse_with_exact_operands() {
    let bytes = fixture(true);
    assert!(inspect_pe_static_imports(&bytes, limits()).is_ok());
    for (caps, error) in [
        (
            [4095, 0, 0],
            PeStaticImportEvidenceError::InputTooLarge {
                length: 4096,
                limit: 4095,
            },
        ),
        (
            [4096, 7, 0],
            PeStaticImportEvidenceError::OutputRowsExceeded { rows: 8, limit: 7 },
        ),
        (
            [4096, 8, 39],
            PeStaticImportEvidenceError::OutputTextExceeded {
                bytes: 40,
                limit: 39,
            },
        ),
    ] {
        assert_eq!(
            inspect_pe_static_imports(
                &bytes,
                PeStaticImportEvidenceLimits {
                    max_input_bytes: caps[0],
                    max_output_rows: caps[1],
                    max_output_text_bytes: caps[2],
                }
            ),
            Err(error)
        );
    }
}

#[test]
fn readable_descriptors_survive_lookup_failure_without_iat_fallback() {
    let mut bytes = fixture(false);
    put32(&mut bytes, 532, 0);
    put32(&mut bytes, 548, 0x1640);
    let caps = PeStaticImportEvidenceLimits {
        max_output_rows: 2,
        max_output_text_bytes: 14,
        ..limits()
    };
    let evidence = inspect_pe_static_imports(&bytes, caps).unwrap();
    bytes.fill(0);
    drop(bytes);
    assert_eq!((evidence.total_rows, evidence.total_text_bytes), (2, 14));
    let descriptors = evidence.descriptors.unwrap();
    assert_eq!(descriptors.len(), 2);
    assert_eq!(descriptors[1].dll_name, "Other");
    assert_eq!(
        descriptors[1].import_lookup_table_rva,
        RelativeVirtualAddress::new(0)
    );
    assert_eq!(
        descriptors[1].import_address_table_rva,
        RelativeVirtualAddress::new(0x1640)
    );
    assert_eq!(
        evidence.lookups,
        Err(PeImportLookupError::LookupTableUnavailable {
            descriptor_index: 1
        })
    );
}

#[test]
fn descriptor_errors_remain_independent_and_failed_results_cost_zero_output() {
    let mut bytes = fixture(false);
    put32(&mut bytes, 512, 0);
    bytes[1568] = 0;
    let caps = PeStaticImportEvidenceLimits {
        max_output_rows: 0,
        max_output_text_bytes: 0,
        ..limits()
    };
    let evidence = inspect_pe_static_imports(&bytes, caps).unwrap();
    let error = PeImportError::EmptyDllName {
        descriptor_index: 1,
        name_rva: RelativeVirtualAddress::new(0x1420),
    };
    assert_eq!((evidence.total_rows, evidence.total_text_bytes), (0, 0));
    assert_eq!(evidence.descriptors, Err(error));
    assert_eq!(
        evidence.lookups,
        Err(PeImportLookupError::Descriptors(error))
    );
    bytes[0] = 0;
    let evidence = inspect_pe_static_imports(&bytes, caps).unwrap();
    let error = PeImportError::Base(PeRvaError::Parse(PeHeaderError::InvalidDosSignature {
        offset: FileOffset::new(0),
    }));
    assert_eq!(evidence.descriptors, Err(error));
    assert_eq!(
        evidence.lookups,
        Err(PeImportLookupError::Descriptors(error))
    );
    assert_eq!(
        inspect_pe_static_imports(
            &bytes,
            PeStaticImportEvidenceLimits {
                max_input_bytes: 4095,
                ..caps
            }
        ),
        Err(PeStaticImportEvidenceError::InputTooLarge {
            length: 4096,
            limit: 4095
        })
    );
}

#[test]
fn absent_zero_and_terminated_empty_tables_are_empty_success() {
    for plus in [false, true] {
        for mode in 0..3 {
            let mut bytes = fixture(plus);
            let fixed = if plus { 112 } else { 96 };
            match mode {
                0 => put32(&mut bytes, 152 + fixed - 4, 1),
                1 => {
                    put32(&mut bytes, 152 + fixed + 8, 0);
                    put32(&mut bytes, 152 + fixed + 12, 0);
                }
                _ => bytes[512..532].fill(0),
            }
            let evidence = inspect_pe_static_imports(
                &bytes,
                PeStaticImportEvidenceLimits {
                    max_output_rows: 0,
                    max_output_text_bytes: 0,
                    ..limits()
                },
            )
            .unwrap();
            assert_eq!((evidence.total_rows, evidence.total_text_bytes), (0, 0));
            assert!(evidence.descriptors.unwrap().is_empty());
            assert!(evidence.lookups.unwrap().is_empty());
        }
    }
}

#[test]
fn lookup_name_errors_discard_lookup_prefix_without_losing_dll_observations() {
    let mut bytes = fixture(true);
    bytes[2562] = 255;
    let caps = PeStaticImportEvidenceLimits {
        max_output_rows: 2,
        max_output_text_bytes: 14,
        ..limits()
    };
    let evidence = inspect_pe_static_imports(&bytes, caps).unwrap();
    assert_eq!(evidence.descriptors.unwrap().len(), 2);
    assert_eq!(
        evidence.lookups,
        Err(PeImportLookupError::NonAsciiSymbolName {
            descriptor_index: 0,
            entry_index: 0,
            hint_name_rva: RelativeVirtualAddress::new(0x1800),
            offset: 0,
            byte: 255
        })
    );
    assert_eq!(
        inspect_pe_static_imports(
            &bytes,
            PeStaticImportEvidenceLimits {
                max_output_text_bytes: 13,
                ..caps
            }
        ),
        Err(PeStaticImportEvidenceError::OutputTextExceeded {
            bytes: 14,
            limit: 13
        })
    );
}
