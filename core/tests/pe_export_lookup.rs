use ring3_core::{
    FileOffset, PeExportAddressError, PeExportDirectoryError, PeExportLookupError,
    PeExportNameError, PeExportQuery, PeExportSelection, PeExportTarget, PeHeaderError, PeRvaError,
    RelativeVirtualAddress, lookup_pe_export,
};

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn file_offset(rva: u32) -> usize {
    usize::try_from(rva - 0x1000 + 512).unwrap()
}

fn directory(bytes: &mut [u8], plus: bool, rva: u32, size: u32) {
    let slot = if plus { 264 } else { 248 };
    put32(bytes, slot, rva);
    put32(bytes, slot + 4, size);
}

fn section(bytes: &mut [u8], plus: bool, index: usize, fields: [u32; 4]) {
    let table = if plus { 272 } else { 256 };
    for (offset, value) in [8, 12, 16, 20].into_iter().zip(fields) {
        put32(bytes, table + index * 40 + offset, value);
    }
}

fn row(bytes: &mut [u8], index: u32, rva: u32, address: u16) {
    put32(bytes, file_offset(0x5000 + index * 4), rva);
    let offset = file_offset(0x9000 + index * 2);
    bytes[offset..offset + 2].copy_from_slice(&address.to_le_bytes());
}

fn fixture(plus: bool, count: u32) -> Vec<u8> {
    let mut bytes = vec![0; 65_536];
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 60, 128);
    bytes[128..132].copy_from_slice(b"PE\0\0");
    bytes[134..136].copy_from_slice(&2_u16.to_le_bytes());
    let fixed = if plus { 112_u16 } else { 96 };
    bytes[148..150].copy_from_slice(&(fixed + 8).to_le_bytes());
    bytes[152..154].copy_from_slice(&if plus { 0x20b_u16 } else { 0x10b }.to_le_bytes());
    put32(&mut bytes, 212, 512);
    put32(&mut bytes, 152 + usize::from(fixed) - 4, 1);
    directory(&mut bytes, plus, 0x1000, 0x1000);
    section(&mut bytes, plus, 0, [60_000, 0x1000, 60_000, 512]);
    put32(&mut bytes, 528, 7);
    put32(&mut bytes, 532, 3);
    put32(&mut bytes, 536, count);
    put32(&mut bytes, 540, 0x3000);
    put32(&mut bytes, 544, 0x5000);
    put32(&mut bytes, 548, 0x9000);
    put32(&mut bytes, file_offset(0x3004), 0x40_0000);
    put32(&mut bytes, file_offset(0x3008), 0x1100);
    bytes[768..772].copy_from_slice(b"W.F\0");
    let first = file_offset(0xc000);
    bytes[first..first + 5].copy_from_slice(b"zeta\0");
    let second = file_offset(0xc010);
    bytes[second..second + 6].copy_from_slice(b"Alpha\0");
    for index in 0..count.min(4096) {
        row(&mut bytes, index, 0xc000, 0);
    }
    bytes
}

#[test]
fn both_widths_select_exact_named_coordinates_and_raw_targets() {
    for plus in [false, true] {
        for (index, target) in [
            (0, PeExportTarget::Empty),
            (
                1,
                PeExportTarget::Rva(RelativeVirtualAddress::new(0x40_0000)),
            ),
            (
                2,
                PeExportTarget::Forwarder {
                    rva: RelativeVirtualAddress::new(0x1100),
                    text: "W.F",
                },
            ),
        ] {
            let mut bytes = fixture(plus, 1);
            row(&mut bytes, 0, 0xc000, index);
            let PeExportSelection::Selected {
                address,
                name: Some(name),
            } = lookup_pe_export(&bytes, PeExportQuery::Name("zeta")).unwrap()
            else {
                panic!()
            };
            assert_eq!(address.table_index, u32::from(index));
            assert_eq!(address.ordinal, 7 + u32::from(index));
            assert_eq!(
                address.entry_rva,
                RelativeVirtualAddress::new(0x3000 + 4 * u32::from(index))
            );
            assert_eq!(
                address.entry_file_offset,
                FileOffset::new(8704 + 4 * u64::from(index))
            );
            assert_eq!(address.target, target);
            assert_eq!(name.table_index, 0);
            assert_eq!(name.address_index, index);
            assert_eq!(name.name_pointer_rva, RelativeVirtualAddress::new(0x5000));
            assert_eq!(name.name_pointer_file_offset, FileOffset::new(16896));
            assert_eq!(name.ordinal_entry_rva, RelativeVirtualAddress::new(0x9000));
            assert_eq!(name.ordinal_entry_file_offset, FileOffset::new(33280));
            assert_eq!(name.name_rva, RelativeVirtualAddress::new(0xc000));
            assert_eq!(name.name_file_offset, FileOffset::new(45568));
            assert_eq!(name.name, "zeta");
        }
    }
}

#[test]
fn unsorted_duplicate_rows_remain_ambiguous_even_at_the_same_address() {
    for plus in [false, true] {
        for last in [0, 2] {
            let mut bytes = fixture(plus, 3);
            row(&mut bytes, 1, 0xc010, 1);
            row(&mut bytes, 2, 0xc000, last);
            let PeExportSelection::AmbiguousName { matches } =
                lookup_pe_export(&bytes, PeExportQuery::Name("zeta")).unwrap()
            else {
                panic!()
            };
            assert_eq!(matches.len(), 2);
            assert_eq!((matches[0].table_index, matches[0].address_index), (0, 0));
            assert_eq!(
                (matches[1].table_index, matches[1].address_index),
                (2, last)
            );
            assert_eq!(matches[0].name, matches[1].name);
            assert_eq!(matches[1].ordinal_entry_file_offset, FileOffset::new(33284));
        }
    }
}

#[test]
fn names_match_exactly_without_normalization_or_query_lifetime_retention() {
    let bytes = fixture(false, 1);
    for query in ["", "Zeta", "zeta\0", "zéta", "absent"] {
        assert_eq!(
            lookup_pe_export(&bytes, PeExportQuery::Name(query)),
            Ok(PeExportSelection::NameNotFound)
        );
    }
    let result = {
        let query = String::from("zeta");
        lookup_pe_export(&bytes, PeExportQuery::Name(&query)).unwrap()
    };
    let PeExportSelection::Selected {
        name: Some(name), ..
    } = result
    else {
        panic!()
    };
    assert!(std::ptr::eq(
        name.name.as_ptr(),
        bytes[file_offset(0xc000)..].as_ptr()
    ));
}

#[test]
fn ordinal_bounds_use_full_u32_and_keep_raw_targets_without_names() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 0);
        assert_eq!(
            lookup_pe_export(&bytes, PeExportQuery::Ordinal(6)),
            Ok(PeExportSelection::OrdinalBeforeBase {
                ordinal: 6,
                base: 7
            })
        );
        assert_eq!(
            lookup_pe_export(&bytes, PeExportQuery::Ordinal(10)),
            Ok(PeExportSelection::OrdinalOutOfRange {
                ordinal: 10,
                base: 7,
                address_count: 3
            })
        );
        for ordinal in 7..=9 {
            let PeExportSelection::Selected { address, name } =
                lookup_pe_export(&bytes, PeExportQuery::Ordinal(ordinal)).unwrap()
            else {
                panic!()
            };
            assert_eq!(address.ordinal, ordinal);
            assert_eq!(address.table_index, ordinal - 7);
            assert!(name.is_none());
        }
        put32(&mut bytes, 528, u32::MAX);
        put32(&mut bytes, 532, 1);
        let PeExportSelection::Selected {
            address,
            name: None,
        } = lookup_pe_export(&bytes, PeExportQuery::Ordinal(u32::MAX)).unwrap()
        else {
            panic!()
        };
        assert_eq!(address.ordinal, u32::MAX);
        assert_eq!(address.table_index, 0);
        assert_eq!(
            lookup_pe_export(&bytes, PeExportQuery::Ordinal(u32::MAX - 1)),
            Ok(PeExportSelection::OrdinalBeforeBase {
                ordinal: u32::MAX - 1,
                base: u32::MAX
            })
        );
    }
}

#[test]
fn absent_directory_and_present_empty_tables_have_distinct_outcomes() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 0);
        put32(&mut bytes, 532, 0);
        put32(&mut bytes, 540, u32::MAX);
        assert_eq!(
            lookup_pe_export(&bytes, PeExportQuery::Name("zeta")),
            Ok(PeExportSelection::NameNotFound)
        );
        assert_eq!(
            lookup_pe_export(&bytes, PeExportQuery::Ordinal(7)),
            Ok(PeExportSelection::OrdinalOutOfRange {
                ordinal: 7,
                base: 7,
                address_count: 0
            })
        );
        directory(&mut bytes, plus, 0, 0);
        for query in [PeExportQuery::Name("zeta"), PeExportQuery::Ordinal(7)] {
            assert_eq!(
                lookup_pe_export(&bytes, query),
                Ok(PeExportSelection::DirectoryAbsent)
            );
        }
    }
}

#[test]
fn ordinal_path_does_not_parse_malformed_name_metadata() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 1);
        put32(&mut bytes, 536, u32::MAX);
        put32(&mut bytes, 544, u32::MAX);
        assert!(matches!(
            lookup_pe_export(&bytes, PeExportQuery::Ordinal(9)),
            Ok(PeExportSelection::Selected { name: None, .. })
        ));
        assert_eq!(
            lookup_pe_export(&bytes, PeExportQuery::Name("zeta")),
            Err(PeExportLookupError::Names(
                PeExportNameError::NameLimitExceeded {
                    count: u32::MAX,
                    limit: 4096
                }
            ))
        );
    }
}

#[test]
fn complete_required_tables_are_validated_before_a_match_or_range_outcome() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 2);
        row(&mut bytes, 1, 0xc010, 3);
        assert_eq!(
            lookup_pe_export(&bytes, PeExportQuery::Name("zeta")),
            Err(PeExportLookupError::Names(
                PeExportNameError::AddressIndexOutOfRange {
                    entry_index: 1,
                    address_index: 3,
                    address_count: 3
                }
            ))
        );
        assert!(matches!(
            lookup_pe_export(&bytes, PeExportQuery::Ordinal(7)),
            Ok(PeExportSelection::Selected { .. })
        ));
        put32(&mut bytes, 528, u32::MAX);
        let cause = PeExportAddressError::OrdinalOverflow {
            ordinal_base: u32::MAX,
            entry_index: 1,
        };
        assert_eq!(
            lookup_pe_export(&bytes, PeExportQuery::Ordinal(0)),
            Err(PeExportLookupError::Addresses(cause))
        );
        assert_eq!(
            lookup_pe_export(&bytes, PeExportQuery::Name("zeta")),
            Err(PeExportLookupError::Names(PeExportNameError::Addresses(
                cause
            )))
        );
    }
}

#[test]
fn malformed_headers_preserve_the_query_specific_typed_cause() {
    let cause = PeExportAddressError::Directory(PeExportDirectoryError::Base(PeRvaError::Parse(
        PeHeaderError::OutOfBounds {
            offset: FileOffset::new(0),
            needed: 2,
            available: 0,
        },
    )));
    assert_eq!(
        lookup_pe_export(b"", PeExportQuery::Ordinal(0)),
        Err(PeExportLookupError::Addresses(cause))
    );
    assert_eq!(
        lookup_pe_export(b"", PeExportQuery::Name("")),
        Err(PeExportLookupError::Names(PeExportNameError::Addresses(
            cause
        )))
    );
}

#[test]
fn borrowed_forwarder_and_full_ambiguity_limit_preserve_inputs_and_repeats() {
    let bytes = fixture(true, 4096);
    let before = bytes.clone();
    let first = lookup_pe_export(&bytes, PeExportQuery::Name("zeta")).unwrap();
    assert_eq!(
        first,
        lookup_pe_export(&bytes, PeExportQuery::Name("zeta")).unwrap()
    );
    let PeExportSelection::AmbiguousName { matches } = first else {
        panic!()
    };
    assert_eq!(matches.len(), 4096);
    assert_eq!(matches[4095].table_index, 4095);
    assert!(
        matches
            .iter()
            .all(|name| std::ptr::eq(name.name.as_ptr(), bytes[file_offset(0xc000)..].as_ptr()))
    );
    let PeExportSelection::Selected {
        address,
        name: None,
    } = lookup_pe_export(&bytes, PeExportQuery::Ordinal(9)).unwrap()
    else {
        panic!()
    };
    let PeExportTarget::Forwarder { text, .. } = address.target else {
        panic!()
    };
    assert!(std::ptr::eq(text.as_ptr(), bytes[768..].as_ptr()));
    assert_eq!(bytes, before);
}

fn read_corpus_provider(variable: &str, kind: ring3_core::PeKind) -> (std::path::PathBuf, Vec<u8>) {
    let path = std::path::PathBuf::from(
        std::env::var_os(variable).expect("explicit generated provider fixture path"),
    );
    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(bytes.len(), 2048);
    assert_eq!(
        ring3_core::parse_pe_header_prefix(&bytes).unwrap().kind,
        kind
    );
    (path, bytes)
}

fn check_corpus_selection(
    bytes: &[u8],
    query: PeExportQuery<'_>,
    expected: &PeExportSelection<'_>,
) {
    let before = bytes.to_vec();
    let result = lookup_pe_export(bytes, query).unwrap();
    assert_eq!(&result, expected);
    assert_eq!(&lookup_pe_export(bytes, query).unwrap(), expected);
    if let PeExportSelection::Selected { address, name } = result {
        if let Some(name) = name {
            let offset = usize::try_from(name.name_file_offset.get()).unwrap();
            assert!(std::ptr::eq(name.name.as_ptr(), bytes[offset..].as_ptr()));
        }
        if let PeExportTarget::Forwarder { rva, text } = address.target {
            let offset = match rva.get() {
                8295 => 1639,
                8320 => 1664,
                _ => panic!("unexpected corpus forwarder"),
            };
            assert!(std::ptr::eq(text.as_ptr(), bytes[offset..].as_ptr()));
        }
    }
    assert_eq!(bytes, before);
}

fn check_direct_corpus_selection(variable: &str, kind: ring3_core::PeKind, named: bool) {
    use ring3_core::{PeExportAddressEntry, PeExportName};

    let (path, bytes) = read_corpus_provider(variable, kind);
    let ordinal = if named { 1 } else { 32768 };
    let address = PeExportAddressEntry {
        table_index: 0,
        ordinal,
        entry_rva: RelativeVirtualAddress::new(if named { 8247 } else { 8249 }),
        entry_file_offset: FileOffset::new(if named { 1591 } else { 1593 }),
        target: PeExportTarget::Rva(RelativeVirtualAddress::new(4096)),
    };
    check_corpus_selection(
        &bytes,
        PeExportQuery::Ordinal(ordinal),
        &PeExportSelection::Selected {
            address,
            name: None,
        },
    );
    let name = PeExportName {
        table_index: 0,
        name_pointer_rva: RelativeVirtualAddress::new(8251),
        name_pointer_file_offset: FileOffset::new(1595),
        ordinal_entry_rva: RelativeVirtualAddress::new(8255),
        ordinal_entry_file_offset: FileOffset::new(1599),
        address_index: 0,
        name_rva: RelativeVirtualAddress::new(8257),
        name_file_offset: FileOffset::new(1601),
        name: "ring3_probe",
    };
    let expected = if named {
        PeExportSelection::Selected {
            address,
            name: Some(name),
        }
    } else {
        PeExportSelection::NameNotFound
    };
    check_corpus_selection(&bytes, PeExportQuery::Name("ring3_probe"), &expected);
    check_corpus_selection(
        &bytes,
        PeExportQuery::Name("RING3_PROBE"),
        &PeExportSelection::NameNotFound,
    );
    check_corpus_selection(
        &bytes,
        PeExportQuery::Ordinal(ordinal - 1),
        &PeExportSelection::OrdinalBeforeBase {
            ordinal: ordinal - 1,
            base: ordinal,
        },
    );
    check_corpus_selection(
        &bytes,
        PeExportQuery::Ordinal(ordinal + 1),
        &PeExportSelection::OrdinalOutOfRange {
            ordinal: ordinal + 1,
            base: ordinal,
            address_count: 1,
        },
    );
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

fn check_forwarder_corpus_selection(variable: &str, kind: ring3_core::PeKind) {
    use ring3_core::{PeExportAddressEntry, PeExportName};

    let (path, bytes) = read_corpus_provider(variable, kind);
    for (name_index, address_index, name_rva, name_offset, name, target_rva, text) in [
        (
            0,
            0_u16,
            8276,
            1620,
            "by_name",
            8295,
            "OtherModule.ring3_target",
        ),
        (1, 2, 8284, 1628, "by_ordinal", 8320, "OtherModule.#32768"),
    ] {
        let address = PeExportAddressEntry {
            table_index: u32::from(address_index),
            ordinal: 7 + u32::from(address_index),
            entry_rva: RelativeVirtualAddress::new(8252 + 4 * u32::from(address_index)),
            entry_file_offset: FileOffset::new(1596 + 4 * u64::from(address_index)),
            target: PeExportTarget::Forwarder {
                rva: RelativeVirtualAddress::new(target_rva),
                text,
            },
        };
        let row = PeExportName {
            table_index: name_index,
            name_pointer_rva: RelativeVirtualAddress::new(8264 + 4 * name_index),
            name_pointer_file_offset: FileOffset::new(1608 + 4 * u64::from(name_index)),
            ordinal_entry_rva: RelativeVirtualAddress::new(8272 + 2 * name_index),
            ordinal_entry_file_offset: FileOffset::new(1616 + 2 * u64::from(name_index)),
            address_index,
            name_rva: RelativeVirtualAddress::new(name_rva),
            name_file_offset: FileOffset::new(name_offset),
            name,
        };
        check_corpus_selection(
            &bytes,
            PeExportQuery::Name(name),
            &PeExportSelection::Selected {
                address,
                name: Some(row),
            },
        );
        check_corpus_selection(
            &bytes,
            PeExportQuery::Ordinal(address.ordinal),
            &PeExportSelection::Selected {
                address,
                name: None,
            },
        );
    }
    check_corpus_selection(
        &bytes,
        PeExportQuery::Ordinal(8),
        &PeExportSelection::Selected {
            address: PeExportAddressEntry {
                table_index: 1,
                ordinal: 8,
                entry_rva: RelativeVirtualAddress::new(8256),
                entry_file_offset: FileOffset::new(1600),
                target: PeExportTarget::Empty,
            },
            name: None,
        },
    );
    check_corpus_selection(
        &bytes,
        PeExportQuery::Ordinal(6),
        &PeExportSelection::OrdinalBeforeBase {
            ordinal: 6,
            base: 7,
        },
    );
    check_corpus_selection(
        &bytes,
        PeExportQuery::Ordinal(10),
        &PeExportSelection::OrdinalOutOfRange {
            ordinal: 10,
            base: 7,
            address_count: 3,
        },
    );
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

#[test]
#[ignore = "requires explicit RING3_EXPORT_PE32_NAMED_DLL"]
fn generated_pe32_named_export_selection_matches_metadata() {
    check_direct_corpus_selection(
        "RING3_EXPORT_PE32_NAMED_DLL",
        ring3_core::PeKind::Pe32,
        true,
    );
}

#[test]
#[ignore = "requires explicit RING3_EXPORT_PE32PLUS_NAMED_DLL"]
fn generated_pe32plus_named_export_selection_matches_metadata() {
    check_direct_corpus_selection(
        "RING3_EXPORT_PE32PLUS_NAMED_DLL",
        ring3_core::PeKind::Pe32Plus,
        true,
    );
}

#[test]
#[ignore = "requires explicit RING3_EXPORT_PE32_ORDINAL_DLL"]
fn generated_pe32_ordinal_export_selection_matches_metadata() {
    check_direct_corpus_selection(
        "RING3_EXPORT_PE32_ORDINAL_DLL",
        ring3_core::PeKind::Pe32,
        false,
    );
}

#[test]
#[ignore = "requires explicit RING3_EXPORT_PE32PLUS_ORDINAL_DLL"]
fn generated_pe32plus_ordinal_export_selection_matches_metadata() {
    check_direct_corpus_selection(
        "RING3_EXPORT_PE32PLUS_ORDINAL_DLL",
        ring3_core::PeKind::Pe32Plus,
        false,
    );
}

#[test]
#[ignore = "requires explicit RING3_EXPORT_FORWARD_PE32_FIXTURE"]
fn generated_pe32_forwarder_export_selection_matches_metadata() {
    check_forwarder_corpus_selection(
        "RING3_EXPORT_FORWARD_PE32_FIXTURE",
        ring3_core::PeKind::Pe32,
    );
}

#[test]
#[ignore = "requires explicit RING3_EXPORT_FORWARD_PE32PLUS_FIXTURE"]
fn generated_pe32plus_forwarder_export_selection_matches_metadata() {
    check_forwarder_corpus_selection(
        "RING3_EXPORT_FORWARD_PE32PLUS_FIXTURE",
        ring3_core::PeKind::Pe32Plus,
    );
}
