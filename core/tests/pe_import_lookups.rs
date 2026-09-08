use ring3_core::{
    FileOffset, PeImportError, PeImportLookupError, PeImportSymbol, PeKind, PeRvaError,
    RelativeVirtualAddress, parse_pe_import_lookups,
};

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn file_offset(rva: u32) -> usize {
    usize::try_from(rva - 0x1000 + 512).unwrap()
}

fn source(index: u16) -> u32 {
    0x2000 + u32::from(index) * 0x2400
}

fn entry(bytes: &mut [u8], plus: bool, descriptor: u16, index: usize, raw: u64) {
    let width = if plus { 8 } else { 4 };
    let offset = file_offset(source(descriptor)) + index * width;
    bytes[offset..offset + width].copy_from_slice(&raw.to_le_bytes()[..width]);
}

fn fixture(plus: bool, descriptors: u16) -> Vec<u8> {
    let mut bytes = vec![0; 65_536];
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 60, 128);
    bytes[128..132].copy_from_slice(b"PE\0\0");
    bytes[134..136].copy_from_slice(&1_u16.to_le_bytes());
    let fixed = if plus { 112 } else { 96_u16 };
    bytes[148..150].copy_from_slice(&(fixed + 16).to_le_bytes());
    bytes[152..154].copy_from_slice(&if plus { 0x20b_u16 } else { 0x10b }.to_le_bytes());
    put32(&mut bytes, 212, 512);
    put32(&mut bytes, 152 + usize::from(fixed) - 4, 2);
    put32(&mut bytes, 152 + usize::from(fixed) + 8, 0x1000);
    put32(
        &mut bytes,
        152 + usize::from(fixed) + 12,
        (u32::from(descriptors) + 1) * 20,
    );
    let section = 152 + usize::from(fixed) + 16;
    for (offset, value) in [(8, 60_000), (12, 0x1000), (16, 60_000), (20, 512)] {
        put32(&mut bytes, section + offset, value);
    }
    for index in 0..descriptors {
        let record = 512 + usize::from(index) * 20;
        put32(&mut bytes, record, source(index));
        put32(&mut bytes, record + 4, 123);
        put32(&mut bytes, record + 12, 0x1800);
        put32(&mut bytes, record + 16, u32::MAX);
    }
    let dll = file_offset(0x1800);
    bytes[dll..dll + 8].copy_from_slice(b"Own.DLL\0");
    let hint = file_offset(0xe000);
    bytes[hint..hint + 2].copy_from_slice(&0xabcd_u16.to_le_bytes());
    bytes[hint + 2..hint + 11].copy_from_slice(b"Symbol\tA\0");
    bytes
}

fn section(bytes: &mut [u8], plus: bool, index: usize, va: u32, virtual_size: u32, raw_size: u32) {
    let table = if plus { 280 } else { 264 };
    for (offset, value) in [
        (8, virtual_size),
        (12, va),
        (16, raw_size),
        (20, u32::try_from(file_offset(va)).unwrap()),
    ] {
        put32(bytes, table + index * 40 + offset, value);
    }
}

#[test]
fn both_widths_preserve_mixed_symbols_raw_values_and_borrowed_names() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 1);
        let flag = if plus { 1_u64 << 63 } else { 1 << 31 };
        let values = [0xe000, flag, flag | 32768, flag | 65535, 0xe000];
        for (index, value) in values.into_iter().enumerate() {
            entry(&mut bytes, plus, 0, index, value);
        }
        let results = parse_pe_import_lookups(&bytes).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].descriptor.time_date_stamp, 123);
        assert_eq!(
            results[0].descriptor.import_address_table_rva,
            RelativeVirtualAddress::new(u32::MAX)
        );
        assert_eq!(results[0].entries.len(), 5);
        for (index, result) in results[0].entries.iter().enumerate() {
            let offset = u32::try_from(index).unwrap() * if plus { 8 } else { 4 };
            assert_eq!(
                result.lookup_rva,
                RelativeVirtualAddress::new(0x2000 + offset)
            );
            assert_eq!(
                result.lookup_file_offset,
                FileOffset::new(4608 + u64::from(offset))
            );
            assert_eq!(result.raw_value, values[index]);
        }
        let named = PeImportSymbol::ByName {
            hint_name_rva: RelativeVirtualAddress::new(0xe000),
            hint: 0xabcd,
            name: "Symbol\tA",
        };
        assert_eq!(results[0].entries[0].symbol, named);
        assert_eq!(results[0].entries[1].symbol, PeImportSymbol::Ordinal(0));
        assert_eq!(results[0].entries[2].symbol, PeImportSymbol::Ordinal(32768));
        assert_eq!(results[0].entries[3].symbol, PeImportSymbol::Ordinal(65535));
        assert_eq!(results[0].entries[4].symbol, named);
        let PeImportSymbol::ByName { name, .. } = results[0].entries[0].symbol else {
            panic!("expected named metadata")
        };
        assert!(std::ptr::eq(
            name.as_ptr(),
            bytes[file_offset(0xe000) + 2..].as_ptr()
        ));
    }
}

#[test]
fn all_descriptors_validate_before_zero_lookup_is_refused_without_iat_fallback() {
    let mut bytes = fixture(false, 2);
    put32(&mut bytes, 512, 0);
    put32(&mut bytes, 528, source(0));
    entry(&mut bytes, false, 0, 0, 0x8000_8000);
    put32(&mut bytes, 544, 0x1808);
    assert_eq!(
        parse_pe_import_lookups(&bytes),
        Err(PeImportLookupError::Descriptors(
            PeImportError::EmptyDllName {
                descriptor_index: 1,
                name_rva: RelativeVirtualAddress::new(0x1808),
            }
        ))
    );
    put32(&mut bytes, 544, 0x1800);
    assert_eq!(
        parse_pe_import_lookups(&bytes),
        Err(PeImportLookupError::LookupTableUnavailable {
            descriptor_index: 0
        })
    );
}

#[test]
fn empty_tables_and_every_zero_lookup_source_refuse_fallback_consistently() {
    for plus in [false, true] {
        assert!(
            parse_pe_import_lookups(&fixture(plus, 0))
                .unwrap()
                .is_empty()
        );
        assert!(
            parse_pe_import_lookups(&fixture(plus, 1)).unwrap()[0]
                .entries
                .is_empty()
        );
        for timestamp in [0, 123, u32::MAX] {
            for iat in [0, source(0), u32::MAX] {
                let mut bytes = fixture(plus, 1);
                put32(&mut bytes, 512, 0);
                put32(&mut bytes, 516, timestamp);
                put32(&mut bytes, 528, iat);
                assert_eq!(
                    parse_pe_import_lookups(&bytes),
                    Err(PeImportLookupError::LookupTableUnavailable {
                        descriptor_index: 0
                    })
                );
            }
        }
    }
}

#[test]
fn every_reserved_bit_is_rejected_before_narrowing_or_name_reads() {
    for (plus, kind, flag_bit) in [(false, PeKind::Pe32, 31), (true, PeKind::Pe32Plus, 63)] {
        for bit in 16..flag_bit {
            let mut bytes = fixture(plus, 1);
            let raw_value = (1_u64 << flag_bit) | (1_u64 << bit) | 32768;
            entry(&mut bytes, plus, 0, 0, raw_value);
            assert_eq!(
                parse_pe_import_lookups(&bytes),
                Err(PeImportLookupError::InvalidOrdinalEncoding {
                    descriptor_index: 0,
                    entry_index: 0,
                    raw_value,
                    kind
                })
            );
        }
    }
    for bit in 31..63 {
        let mut bytes = fixture(true, 1);
        let raw_value = (1_u64 << bit) | 0xe000;
        entry(&mut bytes, true, 0, 0, raw_value);
        assert_eq!(
            parse_pe_import_lookups(&bytes),
            Err(PeImportLookupError::InvalidNameEncoding {
                descriptor_index: 0,
                entry_index: 0,
                raw_value,
                kind: PeKind::Pe32Plus
            })
        );
    }
    let mut bytes = fixture(true, 1);
    entry(&mut bytes, true, 0, 0, 0x7fff_ffff);
    let hint_name_rva = RelativeVirtualAddress::new(0x7fff_ffff);
    assert_eq!(
        parse_pe_import_lookups(&bytes),
        Err(PeImportLookupError::HintNameRange {
            descriptor_index: 0,
            entry_index: 0,
            hint_name_rva,
            offset: 0,
            cause: PeRvaError::UnmappedRva {
                start: hint_name_rva,
                length: 3
            },
        })
    );
}

#[test]
fn exact_entry_limits_allow_terminators_and_local_limit_precedes_global_and_encoding() {
    for plus in [false, true] {
        let flag = if plus { 1_u64 << 63 } else { 1 << 31 };
        let mut bytes = fixture(plus, 5);
        for descriptor in 0..4 {
            for index in 0..1024 {
                entry(&mut bytes, plus, descriptor, index, flag | 32768);
            }
        }
        let tables = parse_pe_import_lookups(&bytes).unwrap();
        assert_eq!(
            tables
                .iter()
                .map(|table| table.entries.len())
                .collect::<Vec<_>>(),
            [1024, 1024, 1024, 1024, 0]
        );
        entry(&mut bytes, plus, 4, 0, flag | (1 << 16));
        assert_eq!(
            parse_pe_import_lookups(&bytes),
            Err(PeImportLookupError::TotalEntryLimitExceeded {
                descriptor_index: 4,
                entry_index: 0,
                limit: 4096
            })
        );
        entry(&mut bytes, plus, 3, 1024, flag | (1 << 16));
        assert_eq!(
            parse_pe_import_lookups(&bytes),
            Err(PeImportLookupError::EntryLimitExceeded {
                descriptor_index: 3,
                entry_index: 1024,
                limit: 1024
            })
        );
    }
}

#[test]
fn next_full_entry_range_is_checked_before_the_entry_limit() {
    let mut bytes = fixture(false, 1);
    for index in 0..1024 {
        entry(&mut bytes, false, 0, index, 0x8000_0001);
    }
    section(&mut bytes, false, 0, 0x1000, 0x2000, 0x2000);
    let start = RelativeVirtualAddress::new(source(0));
    assert_eq!(
        parse_pe_import_lookups(&bytes),
        Err(PeImportLookupError::LookupRange {
            descriptor_index: 0,
            entry_index: 1024,
            start,
            length: 4100,
            cause: PeRvaError::CrossesRegionBoundary {
                start,
                length: 4100
            },
        })
    );
}

#[test]
fn lookup_prefix_includes_the_terminator_without_stitching_or_tail_reads() {
    let start = RelativeVirtualAddress::new(source(0));
    for (virtual_size, raw_size, cause) in [
        (
            0x1004,
            0x1008,
            PeRvaError::RawPaddingUnsupported {
                start,
                length: 8,
                section_index: 0,
            },
        ),
        (
            0x1008,
            0x1004,
            PeRvaError::NotFileBacked {
                start,
                length: 8,
                section_index: 0,
            },
        ),
    ] {
        let mut bytes = fixture(false, 1);
        entry(&mut bytes, false, 0, 0, 0x8000_0001);
        section(&mut bytes, false, 0, 0x1000, virtual_size, raw_size);
        assert_eq!(
            parse_pe_import_lookups(&bytes),
            Err(PeImportLookupError::LookupRange {
                descriptor_index: 0,
                entry_index: 1,
                start,
                length: 8,
                cause
            })
        );
    }
    for (size, cause) in [
        (
            0x1004,
            PeRvaError::CrossesRegionBoundary { start, length: 8 },
        ),
        (0x1008, PeRvaError::AmbiguousRange { start, length: 8 }),
    ] {
        let mut bytes = fixture(false, 1);
        bytes[134..136].copy_from_slice(&2_u16.to_le_bytes());
        entry(&mut bytes, false, 0, 0, 0x8000_0001);
        section(&mut bytes, false, 0, 0x1000, size, size);
        section(&mut bytes, false, 1, 0x2004, 4, 4);
        assert_eq!(
            parse_pe_import_lookups(&bytes),
            Err(PeImportLookupError::LookupRange {
                descriptor_index: 0,
                entry_index: 1,
                start,
                length: 8,
                cause
            })
        );
    }
}

#[test]
fn hint_and_first_name_byte_must_share_a_whole_conservative_range() {
    let hint_name_rva = RelativeVirtualAddress::new(0xe000);
    for (virtual_size, raw_size, cause) in [
        (
            2,
            3,
            PeRvaError::RawPaddingUnsupported {
                start: hint_name_rva,
                length: 3,
                section_index: 1,
            },
        ),
        (
            3,
            2,
            PeRvaError::NotFileBacked {
                start: hint_name_rva,
                length: 3,
                section_index: 1,
            },
        ),
    ] {
        let mut bytes = fixture(false, 1);
        entry(&mut bytes, false, 0, 0, 0xe000);
        bytes[134..136].copy_from_slice(&2_u16.to_le_bytes());
        section(&mut bytes, false, 0, 0x1000, 0x2000, 0x2000);
        section(&mut bytes, false, 1, 0xe000, virtual_size, raw_size);
        assert_eq!(
            parse_pe_import_lookups(&bytes),
            Err(PeImportLookupError::HintNameRange {
                descriptor_index: 0,
                entry_index: 0,
                hint_name_rva,
                offset: 0,
                cause
            })
        );
    }
    for (size, cause) in [
        (
            2,
            PeRvaError::CrossesRegionBoundary {
                start: hint_name_rva,
                length: 3,
            },
        ),
        (
            3,
            PeRvaError::AmbiguousRange {
                start: hint_name_rva,
                length: 3,
            },
        ),
    ] {
        let mut bytes = fixture(false, 1);
        entry(&mut bytes, false, 0, 0, 0xe000);
        bytes[134..136].copy_from_slice(&3_u16.to_le_bytes());
        section(&mut bytes, false, 0, 0x1000, 0x2000, 0x2000);
        section(&mut bytes, false, 1, 0xe000, size, size);
        section(&mut bytes, false, 2, 0xe002, 9, 9);
        assert_eq!(
            parse_pe_import_lookups(&bytes),
            Err(PeImportLookupError::HintNameRange {
                descriptor_index: 0,
                entry_index: 0,
                hint_name_rva,
                offset: 0,
                cause
            })
        );
    }
}

#[test]
fn empty_and_non_ascii_symbol_names_report_the_first_byte_and_entry() {
    let mut bytes = fixture(false, 1);
    entry(&mut bytes, false, 0, 0, 0x8000_0001);
    entry(&mut bytes, false, 0, 1, 0xe000);
    let hint_name_rva = RelativeVirtualAddress::new(0xe000);
    bytes[file_offset(0xe000) + 2] = 0;
    assert_eq!(
        parse_pe_import_lookups(&bytes),
        Err(PeImportLookupError::EmptySymbolName {
            descriptor_index: 0,
            entry_index: 1,
            hint_name_rva
        })
    );
    bytes[file_offset(0xe000) + 2..file_offset(0xe000) + 5].copy_from_slice(&[b'A', 0xff, 0]);
    assert_eq!(
        parse_pe_import_lookups(&bytes),
        Err(PeImportLookupError::NonAsciiSymbolName {
            descriptor_index: 0,
            entry_index: 1,
            hint_name_rva,
            offset: 1,
            byte: 0xff
        })
    );
}

#[test]
fn symbol_name_limit_includes_nul_but_excludes_hint_bytes() {
    let mut bytes = fixture(false, 1);
    entry(&mut bytes, false, 0, 0, 0xe000);
    let beginning = file_offset(0xe000) + 2;
    bytes[beginning..beginning + 1023].fill(b'A');
    bytes[beginning + 1023] = 0;
    let result = parse_pe_import_lookups(&bytes).unwrap();
    let PeImportSymbol::ByName { hint, name, .. } = result[0].entries[0].symbol else {
        panic!("expected named metadata")
    };
    assert_eq!(hint, 0xabcd);
    assert_eq!(name.len(), 1023);
    bytes[beginning + 1023] = b'A';
    assert_eq!(
        parse_pe_import_lookups(&bytes),
        Err(PeImportLookupError::NameLengthLimitExceeded {
            descriptor_index: 0,
            entry_index: 0,
            hint_name_rva: RelativeVirtualAddress::new(0xe000),
            limit: 1024
        })
    );
}

#[test]
fn total_symbol_name_budget_counts_duplicates_and_precedes_any_hint_read() {
    let mut bytes = fixture(false, 1);
    let beginning = file_offset(0xe000) + 2;
    bytes[beginning..beginning + 1023].fill(b'A');
    bytes[beginning + 1023] = 0;
    for index in 0..64 {
        entry(&mut bytes, false, 0, index, 0xe000);
    }
    assert_eq!(
        parse_pe_import_lookups(&bytes).unwrap()[0].entries.len(),
        64
    );
    entry(&mut bytes, false, 0, 64, 0x7fff_ffff);
    assert_eq!(
        parse_pe_import_lookups(&bytes),
        Err(PeImportLookupError::NameScanBudgetExceeded {
            descriptor_index: 0,
            entry_index: 64,
            hint_name_rva: RelativeVirtualAddress::new(0x7fff_ffff),
            offset: 0,
            limit: 65_536
        })
    );
    let second = file_offset(0xe800) + 2;
    bytes[second..second + 511].fill(b'B');
    bytes[second + 511] = 0;
    entry(&mut bytes, false, 0, 63, 0xe800);
    entry(&mut bytes, false, 0, 64, 0xe000);
    assert_eq!(
        parse_pe_import_lookups(&bytes),
        Err(PeImportLookupError::NameScanBudgetExceeded {
            descriptor_index: 0,
            entry_index: 64,
            hint_name_rva: RelativeVirtualAddress::new(0xe000),
            offset: 512,
            limit: 65_536
        })
    );
    bytes[second..second + 1024].fill(b'B');
    assert_eq!(
        parse_pe_import_lookups(&bytes),
        Err(PeImportLookupError::NameLengthLimitExceeded {
            descriptor_index: 0,
            entry_index: 63,
            hint_name_rva: RelativeVirtualAddress::new(0xe800),
            limit: 1024
        })
    );
    let mut across_dlls = fixture(false, 2);
    across_dlls[beginning..beginning + 1023].fill(b'A');
    across_dlls[beginning + 1023] = 0;
    for descriptor in 0..2 {
        for index in 0..32 {
            entry(&mut across_dlls, false, descriptor, index, 0xe000);
        }
    }
    assert_eq!(parse_pe_import_lookups(&across_dlls).unwrap().len(), 2);
    entry(&mut across_dlls, false, 1, 32, 0x7fff_ffff);
    assert_eq!(
        parse_pe_import_lookups(&across_dlls),
        Err(PeImportLookupError::NameScanBudgetExceeded {
            descriptor_index: 1,
            entry_index: 32,
            hint_name_rva: RelativeVirtualAddress::new(0x7fff_ffff),
            offset: 0,
            limit: 65_536,
        })
    );
}

fn check_real_fixture(
    variable: &str,
    raw: u64,
    symbol: PeImportSymbol<'static>,
    dll_name: &str,
    name_offset: Option<usize>,
) {
    let path =
        std::env::var(variable).expect("an explicit generated lookup fixture path is required");
    let bytes = std::fs::read(path).unwrap();
    let lookups = parse_pe_import_lookups(&bytes).unwrap();
    assert_eq!(lookups.len(), 1);
    assert_eq!(lookups[0].descriptor.dll_name, dll_name);
    assert_eq!(
        lookups[0].descriptor.descriptor_rva,
        RelativeVirtualAddress::new(8192)
    );
    assert_eq!(
        lookups[0].descriptor.import_lookup_table_rva,
        RelativeVirtualAddress::new(8232)
    );
    assert_eq!(lookups[0].entries.len(), 1);
    let result = lookups[0].entries[0];
    assert_eq!(result.lookup_rva, RelativeVirtualAddress::new(8232));
    assert_eq!(result.lookup_file_offset, FileOffset::new(1576));
    assert_eq!(result.raw_value, raw);
    assert_eq!(result.symbol, symbol);
    if let Some(offset) = name_offset {
        let PeImportSymbol::ByName { name, .. } = result.symbol else {
            panic!("expected named metadata")
        };
        assert!(std::ptr::eq(name.as_ptr(), bytes[offset..].as_ptr()));
    }
}

#[test]
#[ignore = "requires the explicit RING3_IMPORT_PE32_FIXTURE path"]
fn generated_pe32_named_lookup_matches_llvm_and_raw_metadata() {
    check_real_fixture(
        "RING3_IMPORT_PE32_FIXTURE",
        8248,
        PeImportSymbol::ByName {
            hint_name_rva: RelativeVirtualAddress::new(8248),
            hint: 0,
            name: "ring3_probe",
        },
        "Ring3Probe.dll",
        Some(1594),
    );
}

#[test]
#[ignore = "requires the explicit RING3_IMPORT_PE32PLUS_FIXTURE path"]
fn generated_pe32plus_named_lookup_matches_llvm_and_raw_metadata() {
    check_real_fixture(
        "RING3_IMPORT_PE32PLUS_FIXTURE",
        8264,
        PeImportSymbol::ByName {
            hint_name_rva: RelativeVirtualAddress::new(8264),
            hint: 0,
            name: "ring3_probe",
        },
        "Ring3Probe.dll",
        Some(1610),
    );
}

#[test]
#[ignore = "requires the explicit RING3_ORDINAL_PE32_FIXTURE path"]
fn generated_pe32_ordinal_lookup_preserves_bit15() {
    check_real_fixture(
        "RING3_ORDINAL_PE32_FIXTURE",
        0x8000_8000,
        PeImportSymbol::Ordinal(32768),
        "Ring3Ordinal.dll",
        None,
    );
}

#[test]
#[ignore = "requires the explicit RING3_ORDINAL_PE32PLUS_FIXTURE path"]
fn generated_pe32plus_ordinal_lookup_preserves_bit15_and_raw_bit63() {
    check_real_fixture(
        "RING3_ORDINAL_PE32PLUS_FIXTURE",
        0x8000_0000_0000_8000,
        PeImportSymbol::Ordinal(32768),
        "Ring3Ordinal.dll",
        None,
    );
}
