use super::{entry, file_offset, fixture, put32, section, source};
use ring3_core::{
    FileOffset, PeImportError, PeImportLookupError, PeImportLookupObservationError,
    PeImportLookupSource, PeImportSymbol, PeKind, PeObservedImportLookup, PeRvaError,
    RelativeVirtualAddress, parse_pe_import_lookups, parse_pe_import_lookups_with_iat_fallback,
};

fn fallback(bytes: &mut [u8], index: u16) {
    let record = 512 + usize::from(index) * 20;
    put32(bytes, record, 0);
    put32(bytes, record + 16, source(index));
}

#[test]
fn fallback_preserves_source_raw_coordinates_names_and_input_for_both_widths() {
    for plus in [false, true] {
        let flag = if plus { 1_u64 << 63 } else { 1 << 31 };
        for timestamp in [0, 123, u32::MAX] {
            let mut bytes = fixture(plus, 1);
            fallback(&mut bytes, 0);
            put32(&mut bytes, 516, timestamp);
            put32(&mut bytes, 520, u32::MAX);
            for (index, value) in [0xe000, flag, flag | 32768, flag | 65535, 0xe000]
                .into_iter()
                .enumerate()
            {
                entry(&mut bytes, plus, 0, index, value);
            }
            let before = bytes.clone();
            let tables = parse_pe_import_lookups_with_iat_fallback(&bytes).unwrap();
            let table = &tables[0];
            assert_eq!(table.source, PeImportLookupSource::FirstThunkFallback);
            assert_eq!(table.descriptor.import_lookup_table_rva.get(), 0);
            assert_eq!(table.descriptor.import_address_table_rva.get(), 8192);
            assert_eq!(table.descriptor.time_date_stamp, timestamp);
            assert_eq!(table.descriptor.forwarder_chain, u32::MAX);
            assert_eq!(table.entries.len(), 5);
            assert!(std::ptr::eq(
                table.descriptor.dll_name.as_ptr(),
                bytes[file_offset(0x1800)..].as_ptr()
            ));
            let width = if plus { 8 } else { 4 };
            for (index, value) in table.entries.iter().enumerate() {
                let offset = u32::try_from(index).unwrap() * width;
                assert_eq!(value.lookup_rva.get(), 8192 + offset);
                assert_eq!(
                    value.lookup_file_offset,
                    FileOffset::new(4608 + u64::from(offset))
                );
                if let PeImportSymbol::ByName {
                    hint,
                    name,
                    hint_name_rva,
                } = value.symbol
                {
                    assert_eq!(
                        (hint, name, hint_name_rva.get()),
                        (0xabcd, "Symbol\tA", 0xe000)
                    );
                    assert!(std::ptr::eq(
                        name.as_ptr(),
                        bytes[file_offset(0xe000) + 2..].as_ptr()
                    ));
                }
            }
            assert_eq!(table.entries[1].symbol, PeImportSymbol::Ordinal(0));
            assert_eq!(table.entries[2].symbol, PeImportSymbol::Ordinal(32768));
            assert_eq!(table.entries[3].symbol, PeImportSymbol::Ordinal(65535));
            assert_eq!(table.entries[3].raw_value, flag | 65535);
            assert_eq!(table.entries[0].symbol, table.entries[4].symbol);
            assert_eq!(bytes, before);
            assert_eq!(
                parse_pe_import_lookups(&bytes),
                Err(PeImportLookupError::LookupTableUnavailable {
                    descriptor_index: 0
                })
            );
        }
    }
}

#[test]
fn nonzero_oft_wins_even_when_aliased_or_malformed_and_never_rescues() {
    for plus in [false, true] {
        for iat in [0, source(0), u32::MAX] {
            let mut bytes = fixture(plus, 1);
            put32(&mut bytes, 528, iat);
            let tables = parse_pe_import_lookups_with_iat_fallback(&bytes).unwrap();
            assert_eq!(tables[0].source, PeImportLookupSource::OriginalFirstThunk);
            assert!(tables[0].entries.is_empty());
            assert_eq!(
                tables[0].descriptor,
                parse_pe_import_lookups(&bytes).unwrap()[0].descriptor
            );
        }
        let mut bytes = fixture(plus, 1);
        put32(&mut bytes, 512, u32::MAX);
        put32(&mut bytes, 528, source(0));
        let start = RelativeVirtualAddress::new(u32::MAX);
        let length = if plus { 8 } else { 4 };
        let cause = PeImportLookupError::LookupRange {
            descriptor_index: 0,
            entry_index: 0,
            start,
            length,
            cause: PeRvaError::RvaRangeOverflow { start, length },
        };
        assert_eq!(
            parse_pe_import_lookups_with_iat_fallback(&bytes),
            Err(PeImportLookupObservationError::Lookup {
                descriptor_index: 0,
                source: PeImportLookupSource::OriginalFirstThunk,
                start,
                cause
            })
        );
        assert_eq!(parse_pe_import_lookups(&bytes), Err(cause));
    }
}

#[test]
fn all_descriptors_precede_selection_and_later_unavailable_discards_success() {
    let mut bytes = fixture(false, 2);
    put32(&mut bytes, 512, 0);
    put32(&mut bytes, 528, 0);
    put32(&mut bytes, 544, 0x1808);
    assert_eq!(
        parse_pe_import_lookups_with_iat_fallback(&bytes),
        Err(PeImportLookupObservationError::Descriptors(
            PeImportError::EmptyDllName {
                descriptor_index: 1,
                name_rva: RelativeVirtualAddress::new(0x1808)
            }
        ))
    );
    put32(&mut bytes, 544, 0x1800);
    assert_eq!(
        parse_pe_import_lookups_with_iat_fallback(&bytes),
        Err(PeImportLookupObservationError::LookupTableUnavailable {
            descriptor_index: 0
        })
    );
    put32(&mut bytes, 512, source(0));
    entry(&mut bytes, false, 0, 0, 0x8000_0001);
    put32(&mut bytes, 532, 0);
    put32(&mut bytes, 548, 0);
    assert_eq!(
        parse_pe_import_lookups_with_iat_fallback(&bytes),
        Err(PeImportLookupObservationError::LookupTableUnavailable {
            descriptor_index: 1
        })
    );
}

#[test]
fn mixed_sources_share_entry_budgets_with_terminators_and_local_precedence() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 5);
        for i in [1, 3, 4] {
            fallback(&mut bytes, i);
        }
        let flag = if plus { 1_u64 << 63 } else { 1 << 31 };
        for i in 0..4 {
            for j in 0..1024 {
                entry(&mut bytes, plus, i, j, flag | 32768);
            }
        }
        let tables = parse_pe_import_lookups_with_iat_fallback(&bytes).unwrap();
        assert_eq!(
            tables.iter().map(|t| t.entries.len()).collect::<Vec<_>>(),
            [1024, 1024, 1024, 1024, 0]
        );
        assert_eq!(
            tables.iter().map(|t| t.source).collect::<Vec<_>>(),
            [
                PeImportLookupSource::OriginalFirstThunk,
                PeImportLookupSource::FirstThunkFallback,
                PeImportLookupSource::OriginalFirstThunk,
                PeImportLookupSource::FirstThunkFallback,
                PeImportLookupSource::FirstThunkFallback
            ]
        );
        entry(&mut bytes, plus, 4, 0, flag | 65536);
        assert_eq!(
            parse_pe_import_lookups_with_iat_fallback(&bytes),
            Err(PeImportLookupObservationError::Lookup {
                descriptor_index: 4,
                source: PeImportLookupSource::FirstThunkFallback,
                start: RelativeVirtualAddress::new(source(4)),
                cause: PeImportLookupError::TotalEntryLimitExceeded {
                    descriptor_index: 4,
                    entry_index: 0,
                    limit: 4096
                }
            })
        );
        entry(&mut bytes, plus, 3, 1024, flag | 65536);
        assert_eq!(
            parse_pe_import_lookups_with_iat_fallback(&bytes),
            Err(PeImportLookupObservationError::Lookup {
                descriptor_index: 3,
                source: PeImportLookupSource::FirstThunkFallback,
                start: RelativeVirtualAddress::new(source(3)),
                cause: PeImportLookupError::EntryLimitExceeded {
                    descriptor_index: 3,
                    entry_index: 1024,
                    limit: 1024
                }
            })
        );
    }
}

#[test]
fn mixed_sources_charge_repeated_names_before_an_unmapped_hint_read() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 2);
        fallback(&mut bytes, 1);
        let first = file_offset(0xe000) + 2;
        bytes[first..first + 1023].fill(b'A');
        bytes[first + 1023] = 0;
        for i in 0..2 {
            for j in 0..32 {
                entry(&mut bytes, plus, i, j, 0xe000);
            }
        }
        assert_eq!(
            parse_pe_import_lookups_with_iat_fallback(&bytes)
                .unwrap()
                .len(),
            2
        );
        entry(&mut bytes, plus, 1, 32, 0x7fff_ffff);
        assert_eq!(
            parse_pe_import_lookups_with_iat_fallback(&bytes),
            Err(PeImportLookupObservationError::Lookup {
                descriptor_index: 1,
                source: PeImportLookupSource::FirstThunkFallback,
                start: RelativeVirtualAddress::new(source(1)),
                cause: PeImportLookupError::NameScanBudgetExceeded {
                    descriptor_index: 1,
                    entry_index: 32,
                    hint_name_rva: RelativeVirtualAddress::new(0x7fff_ffff),
                    offset: 0,
                    limit: 65_536
                }
            })
        );
    }
}

#[test]
fn iat_prefix_range_errors_preserve_source_base_and_nested_operands() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 1);
        fallback(&mut bytes, 0);
        let width = if plus { 8 } else { 4 };
        let flag = if plus { 1_u64 << 63 } else { 1 << 31 };
        entry(&mut bytes, plus, 0, 0, flag | 1);
        section(
            &mut bytes,
            plus,
            0,
            0x1000,
            0x1000 + width,
            0x1000 + 2 * width,
        );
        let start = RelativeVirtualAddress::new(source(0));
        let length = 2 * width;
        assert_eq!(
            parse_pe_import_lookups_with_iat_fallback(&bytes),
            Err(PeImportLookupObservationError::Lookup {
                descriptor_index: 0,
                source: PeImportLookupSource::FirstThunkFallback,
                start,
                cause: PeImportLookupError::LookupRange {
                    descriptor_index: 0,
                    entry_index: 1,
                    start,
                    length,
                    cause: PeRvaError::RawPaddingUnsupported {
                        start,
                        length,
                        section_index: 0
                    }
                }
            })
        );
    }
}

#[test]
fn lookup_shaped_addresses_decode_as_facts_and_reserved_bits_refuse() {
    for (plus, kind, flag) in [
        (false, PeKind::Pe32, 1_u64 << 31),
        (true, PeKind::Pe32Plus, 1 << 63),
    ] {
        let mut bytes = fixture(plus, 1);
        fallback(&mut bytes, 0);
        entry(&mut bytes, plus, 0, 0, flag | 1);
        let table = parse_pe_import_lookups_with_iat_fallback(&bytes)
            .unwrap()
            .remove(0);
        assert_eq!(table.entries[0].symbol, PeImportSymbol::Ordinal(1));
        let raw_value = flag | 0x0001_0001;
        entry(&mut bytes, plus, 0, 0, raw_value);
        assert_eq!(
            parse_pe_import_lookups_with_iat_fallback(&bytes),
            Err(PeImportLookupObservationError::Lookup {
                descriptor_index: 0,
                source: PeImportLookupSource::FirstThunkFallback,
                start: RelativeVirtualAddress::new(source(0)),
                cause: PeImportLookupError::InvalidOrdinalEncoding {
                    descriptor_index: 0,
                    entry_index: 0,
                    raw_value,
                    kind
                }
            })
        );
    }
}

fn check_compiled_borrows(bytes: &[u8], table: &PeObservedImportLookup<'_>) {
    let offset = |rva: u32| usize::try_from(rva - 8192 + 1536).unwrap();
    assert!(std::ptr::eq(
        table.descriptor.dll_name.as_ptr(),
        bytes[offset(table.descriptor.name_rva.get())..].as_ptr()
    ));
    if let PeImportSymbol::ByName {
        hint_name_rva,
        name,
        ..
    } = table.entries[0].symbol
    {
        assert!(std::ptr::eq(
            name.as_ptr(),
            bytes[offset(hint_name_rva.get()) + 2..].as_ptr()
        ));
    }
}

pub(super) fn check_compiled_fixture(bytes: &[u8], mut expected: PeObservedImportLookup<'_>) {
    let original = bytes.to_vec();
    let observed = parse_pe_import_lookups_with_iat_fallback(bytes).unwrap();
    assert_eq!(observed, [expected.clone()]);
    assert_eq!(
        parse_pe_import_lookups_with_iat_fallback(bytes).unwrap(),
        observed
    );
    check_compiled_borrows(bytes, &observed[0]);
    assert_eq!(bytes, original);

    // this copy is a four-byte descriptor mutation, not a linker mode.
    let mut zero_oft_copy = bytes.to_vec();
    zero_oft_copy[1536..1540].fill(0);
    assert_eq!(zero_oft_copy[..1536], bytes[..1536]);
    assert_eq!(zero_oft_copy[1536..1540], [0; 4]);
    assert_eq!(zero_oft_copy[1540..], bytes[1540..]);
    let before = zero_oft_copy.clone();
    expected.descriptor.import_lookup_table_rva = RelativeVirtualAddress::new(0);
    expected.source = PeImportLookupSource::FirstThunkFallback;
    expected.entries[0].lookup_rva = expected.descriptor.import_address_table_rva;
    expected.entries[0].lookup_file_offset = FileOffset::new(
        1536 + u64::from(expected.descriptor.import_address_table_rva.get() - 8192),
    );
    let observed = parse_pe_import_lookups_with_iat_fallback(&zero_oft_copy).unwrap();
    assert_eq!(observed, [expected]);
    assert_eq!(
        parse_pe_import_lookups_with_iat_fallback(&zero_oft_copy).unwrap(),
        observed
    );
    check_compiled_borrows(&zero_oft_copy, &observed[0]);
    assert_eq!(zero_oft_copy, before);
    assert_eq!(bytes, original);
    assert_eq!(
        parse_pe_import_lookups(&zero_oft_copy),
        Err(PeImportLookupError::LookupTableUnavailable {
            descriptor_index: 0
        })
    );
}
