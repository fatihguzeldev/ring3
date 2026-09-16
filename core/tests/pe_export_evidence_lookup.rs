use ring3_core::{
    FileOffset, PeExportAddressError, PeExportEvidence, PeExportEvidenceLimits,
    PeExportEvidenceLookupError as Error, PeExportEvidenceLookupLimits, PeExportLookupError,
    PeExportNameError, PeExportQuery, PeExportSelection, PeExportTarget, PeOwnedExportAddressTable,
    PeOwnedExportNameTable, PeOwnedExportTarget, RelativeVirtualAddress, inspect_pe_exports,
    lookup_pe_export, lookup_pe_export_evidence,
};
use ring3_core::{PeExportBatchError, PeExportBatchLimits, lookup_pe_export_evidence_batch};

fn batch_limits(queries: u64, rows: u64) -> PeExportBatchLimits {
    PeExportBatchLimits {
        max_queries: queries,
        max_selection_rows: rows,
    }
}

#[test]
fn evidence_batch_preserves_order_and_metadata_after_image_and_queries_drop() {
    for plus in [false, true] {
        let bytes = fixture(plus);
        let queries = [
            PeExportQuery::Name("Alpha"),
            PeExportQuery::Name("zeta\t\x01\"\\\x7f"),
            PeExportQuery::Name("Alias"),
            PeExportQuery::Name("alpha"),
            PeExportQuery::Ordinal(0xffff_fff9),
            PeExportQuery::Ordinal(0xffff_fffb),
            PeExportQuery::Ordinal(0xffff_fff8),
            PeExportQuery::Ordinal(u32::MAX),
        ];
        let expected: Vec<_> = queries
            .iter()
            .map(|&q| format!("{:?}", lookup_pe_export(&bytes, q).map_err(Error::Reader)))
            .collect();
        let evidence = inspect_pe_exports(&bytes, collection_limits()).unwrap();
        drop(bytes);
        let before = evidence.clone();
        let batch = lookup_pe_export_evidence_batch(
            &evidence,
            &queries,
            limits(10, 60),
            batch_limits(8, 6),
        )
        .unwrap();
        assert_eq!(batch.selection_rows, 6);
        assert_eq!(
            batch
                .selections
                .iter()
                .map(|s| format!("{s:?}"))
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(
            lookup_pe_export_evidence_batch(
                &evidence,
                &queries,
                limits(10, 60),
                batch_limits(8, 5)
            ),
            Err(PeExportBatchError::SelectionRowsExceeded {
                index: 5,
                total: 6,
                limit: 5
            })
        );
        assert_eq!(
            lookup_pe_export_evidence_batch(
                &evidence,
                &queries,
                limits(10, 60),
                batch_limits(8, 6)
            ),
            Ok(batch)
        );
        let independent = {
            let query = String::from("Alias");
            lookup_pe_export_evidence_batch(
                &evidence,
                &[PeExportQuery::Name(&query)],
                limits(10, 60),
                batch_limits(1, 1),
            )
            .unwrap()
        };
        assert_eq!(independent.selection_rows, 1);
        assert!(matches!(
            independent.selections[0],
            Ok(PeExportSelection::Selected {
                address: ring3_core::PeExportAddressEntry {
                    target: PeExportTarget::Forwarder {
                        text: "M.#00032768",
                        ..
                    },
                    ..
                },
                ..
            })
        ));
        assert_eq!(evidence, before);
    }
}

#[test]
fn evidence_batch_count_and_empty_admission_precede_invalid_evidence() {
    let mut evidence = observed();
    names(&mut evidence).entries[3].address_index = u16::MAX;
    let queries = [
        PeExportQuery::Name("Alpha"),
        PeExportQuery::Ordinal(0xffff_fff9),
    ];
    assert_eq!(
        lookup_pe_export_evidence_batch(&evidence, &queries, limits(0, 0), batch_limits(1, 0)),
        Err(PeExportBatchError::QueryCountExceeded { count: 2, limit: 1 })
    );
    let empty =
        lookup_pe_export_evidence_batch(&evidence, &[], limits(0, 0), batch_limits(0, 0)).unwrap();
    assert_eq!(empty.selection_rows, 0);
    assert!(empty.selections.is_empty());
}

#[test]
fn evidence_batch_retains_per_query_limits_and_late_structure_errors() {
    let mut evidence = observed();
    evidence.total_rows = 0;
    evidence.total_text_bytes = 0;
    let queries = [
        PeExportQuery::Name("Alpha"),
        PeExportQuery::Ordinal(0xffff_fff9),
    ];
    for (query_limits, error) in [
        (limits(6, 32), Error::RowsExceeded { rows: 10, limit: 6 }),
        (
            limits(10, 32),
            Error::TextExceeded {
                bytes: 60,
                limit: 32,
            },
        ),
    ] {
        let batch =
            lookup_pe_export_evidence_batch(&evidence, &queries, query_limits, batch_limits(2, 1))
                .unwrap();
        assert_eq!(batch.selection_rows, 1);
        assert_eq!(batch.selections[0], Err(error));
        assert!(matches!(
            batch.selections[1],
            Ok(PeExportSelection::Selected { name: None, .. })
        ));
    }
    names(&mut evidence).entries[3].address_index = 6;
    let batch =
        lookup_pe_export_evidence_batch(&evidence, &queries, limits(10, 60), batch_limits(2, 1))
            .unwrap();
    assert_eq!(
        batch.selections[0],
        Err(Error::NameAddressIndexOutOfRange {
            entry_index: 3,
            address_index: 6,
            address_count: 6
        })
    );
    assert_eq!(batch.selection_rows, 1);
    assert!(matches!(
        batch.selections[1],
        Ok(PeExportSelection::Selected { name: None, .. })
    ));
}

#[test]
fn evidence_batch_absence_and_reader_errors_cost_no_selection_rows() {
    let mut evidence = observed();
    let error = PeExportNameError::NameUnavailable { entry_index: 3 };
    evidence.names = Err(error);
    evidence.addresses = Ok(None);
    let queries = [
        PeExportQuery::Name("Alpha"),
        PeExportQuery::Ordinal(1),
        PeExportQuery::Name("Alpha"),
    ];
    let batch =
        lookup_pe_export_evidence_batch(&evidence, &queries, limits(0, 0), batch_limits(3, 0))
            .unwrap();
    assert_eq!(batch.selection_rows, 0);
    assert_eq!(
        batch.selections,
        vec![
            Err(Error::Reader(PeExportLookupError::Names(error))),
            Ok(PeExportSelection::DirectoryAbsent),
            Err(Error::Reader(PeExportLookupError::Names(error))),
        ]
    );
}

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
    bytes[148..150].copy_from_slice(&(fixed + 8).to_le_bytes());
    bytes[152..154].copy_from_slice(&if plus { 0x20b_u16 } else { 0x10b }.to_le_bytes());
    put32(&mut bytes, 212, 512);
    put32(&mut bytes, 152 + usize::from(fixed) - 4, 1);
    put32(&mut bytes, 152 + usize::from(fixed), 0x1000);
    put32(&mut bytes, 156 + usize::from(fixed), 0x100);
    for (offset, value) in [(8, 3584), (12, 0x1000), (16, 3584), (20, 512)] {
        put32(&mut bytes, 160 + usize::from(fixed) + offset, value);
    }
    for (offset, value) in [
        (0, u32::MAX),
        (4, 0xabcd_ef12),
        (12, u32::MAX),
        (16, 0xffff_fff9),
        (20, 6),
        (24, 4),
        (28, 0x1200),
        (32, 0x1300),
        (36, 0x1340),
    ] {
        put32(&mut bytes, 512 + offset, value);
    }
    bytes[520..522].copy_from_slice(&u16::MAX.to_le_bytes());
    bytes[522..524].copy_from_slice(&32768_u16.to_le_bytes());
    bytes[640..652].copy_from_slice(b"M.#00032768\0");
    bytes[672..683].copy_from_slice(b"A.B.C\t\x01\"\\\x7f\0");
    for (i, target) in [0, 1, 0x1080, u32::MAX, 0x10a0, 0x1080]
        .into_iter()
        .enumerate()
    {
        put32(&mut bytes, 1024 + i * 4, target);
    }
    for (i, (name, index)) in [(0x1400, 5_u16), (0x1440, 1), (0x1400, 2), (0x1480, 2)]
        .into_iter()
        .enumerate()
    {
        put32(&mut bytes, 1280 + i * 4, name);
        bytes[1344 + i * 2..1346 + i * 2].copy_from_slice(&index.to_le_bytes());
    }
    bytes[1536..1546].copy_from_slice(b"zeta\t\x01\"\\\x7f\0");
    bytes[1600..1606].copy_from_slice(b"Alpha\0");
    bytes[1664..1670].copy_from_slice(b"Alias\0");
    bytes
}

fn collection_limits() -> PeExportEvidenceLimits {
    PeExportEvidenceLimits {
        max_input_bytes: 4096,
        max_output_rows: 16,
        max_output_text_bytes: 92,
    }
}

fn limits(rows: u64, text: u64) -> PeExportEvidenceLookupLimits {
    PeExportEvidenceLookupLimits {
        max_table_rows: rows,
        max_table_text_bytes: text,
    }
}
fn observed() -> PeExportEvidence {
    inspect_pe_exports(&fixture(false), collection_limits()).unwrap()
}
fn addresses(e: &mut PeExportEvidence) -> &mut PeOwnedExportAddressTable {
    e.addresses.as_mut().unwrap().as_mut().unwrap()
}
fn names(e: &mut PeExportEvidence) -> &mut PeOwnedExportNameTable {
    e.names.as_mut().unwrap().as_mut().unwrap()
}
fn query<'e>(
    e: &'e PeExportEvidence,
    q: PeExportQuery<'_>,
) -> Result<PeExportSelection<'e>, Error> {
    lookup_pe_export_evidence(e, q, limits(20, 120))
}

#[test]
fn producer_queries_preserve_complete_results_after_bytes_are_released() {
    for plus in [false, true] {
        for corrupt in [None, Some(1280), Some(1292), Some(1024 + 5 * 4)] {
            let mut bytes = fixture(plus);
            if let Some(offset) = corrupt {
                put32(&mut bytes, offset, if offset == 1044 { 0x10c0 } else { 0 });
            }
            let queries = [
                PeExportQuery::Name("Alpha"),
                PeExportQuery::Name("alpha"),
                PeExportQuery::Name("zeta\t\x01\"\\\x7f"),
                PeExportQuery::Name("Alias"),
                PeExportQuery::Ordinal(0xffff_fff8),
                PeExportQuery::Ordinal(0xffff_fffb),
                PeExportQuery::Ordinal(u32::MAX),
            ];
            let expected: Vec<_> = queries
                .iter()
                .map(|q| format!("{:?}", lookup_pe_export(&bytes, *q).map_err(Error::Reader)))
                .collect();
            let evidence = inspect_pe_exports(&bytes, collection_limits()).unwrap();
            bytes.fill(0xee);
            drop(bytes);
            for (q, want) in queries.into_iter().zip(expected).rev() {
                assert_eq!(format!("{:?}", query(&evidence, q)), want);
            }
        }
    }
}

#[test]
fn required_views_preserve_reader_errors_absence_and_present_empty() {
    let mut e = observed();
    let ne = PeExportNameError::NameUnavailable { entry_index: 3 };
    let ae = PeExportAddressError::AddressTableUnavailable { count: 6 };
    e.names = Err(ne);
    assert_eq!(
        lookup_pe_export_evidence(&e, PeExportQuery::Name("Alpha"), limits(0, 0)),
        Err(Error::Reader(PeExportLookupError::Names(ne)))
    );
    assert!(matches!(
        query(&e, PeExportQuery::Ordinal(0xffff_fffa)),
        Ok(PeExportSelection::Selected { .. })
    ));
    e.addresses = Err(ae);
    assert_eq!(
        lookup_pe_export_evidence(&e, PeExportQuery::Ordinal(1), limits(0, 0)),
        Err(Error::Reader(PeExportLookupError::Addresses(ae)))
    );
    e.names = Ok(None);
    e.addresses = Ok(None);
    for q in [PeExportQuery::Name("Alpha"), PeExportQuery::Ordinal(1)] {
        assert_eq!(
            lookup_pe_export_evidence(&e, q, limits(0, 0)),
            Ok(PeExportSelection::DirectoryAbsent)
        );
    }
    let mut e = observed();
    addresses(&mut e).entries.clear();
    addresses(&mut e).directory.address_table_entries = 0;
    names(&mut e).entries.clear();
    names(&mut e).addresses.entries.clear();
    names(&mut e).addresses.directory.address_table_entries = 0;
    names(&mut e).addresses.directory.number_of_name_pointers = 0;
    assert_eq!(
        lookup_pe_export_evidence(&e, PeExportQuery::Name("Alpha"), limits(0, 0)),
        Ok(PeExportSelection::NameNotFound)
    );
    assert_eq!(
        lookup_pe_export_evidence(&e, PeExportQuery::Ordinal(0xffff_fffa), limits(0, 0)),
        Ok(PeExportSelection::OrdinalOutOfRange {
            ordinal: 0xffff_fffa,
            base: 0xffff_fff9,
            address_count: 0
        })
    );
}

#[test]
fn actual_query_view_budgets_ignore_public_totals_and_count_all_text() {
    for totals in [0, u64::MAX] {
        let mut e = observed();
        e.total_rows = totals;
        e.total_text_bytes = totals;
        for (q, rows, text) in [
            (PeExportQuery::Name("Alpha"), 10, 60),
            (PeExportQuery::Ordinal(0xffff_fffa), 6, 32),
        ] {
            assert!(matches!(
                lookup_pe_export_evidence(&e, q, limits(rows, text)),
                Ok(PeExportSelection::Selected { .. })
            ));
            assert_eq!(
                lookup_pe_export_evidence(&e, q, limits(rows - 1, 0)),
                Err(Error::RowsExceeded {
                    rows,
                    limit: rows - 1
                })
            );
            assert_eq!(
                lookup_pe_export_evidence(&e, q, limits(rows, text - 1)),
                Err(Error::TextExceeded {
                    bytes: text,
                    limit: text - 1
                })
            );
            assert_eq!(
                lookup_pe_export_evidence(&e, q, limits(rows, 0)),
                Err(Error::TextExceeded {
                    bytes: text,
                    limit: 0
                })
            );
        }
    }
}

#[test]
fn divergent_views_and_unrelated_fields_remain_independent() {
    let mut e = observed();
    e.directory = Ok(None);
    names(&mut e).addresses.entries[1].target =
        PeOwnedExportTarget::Rva(RelativeVirtualAddress::new(77));
    for (q, rva) in [
        (PeExportQuery::Name("Alpha"), 77),
        (PeExportQuery::Ordinal(0xffff_fffa), 1),
    ] {
        let PeExportSelection::Selected { address, .. } = query(&e, q).unwrap() else {
            panic!();
        };
        assert_eq!(
            address.target,
            PeExportTarget::Rva(RelativeVirtualAddress::new(rva))
        );
    }
    e.addresses = Err(PeExportAddressError::AddressTableUnavailable { count: 6 });
    assert!(query(&e, PeExportQuery::Name("Alpha")).is_ok());
    let mut e = observed();
    addresses(&mut e).directory.number_of_name_pointers = u32::MAX;
    names(&mut e).entries[3].address_index = 6;
    assert!(query(&e, PeExportQuery::Ordinal(0xffff_fffa)).is_ok());
}

#[test]
fn every_address_is_validated_before_selection_or_range_shortcuts() {
    let ordinal = PeExportQuery::Ordinal(0xffff_fffa);
    let mut e = observed();
    addresses(&mut e).entries.pop();
    assert_eq!(
        query(&e, ordinal),
        Err(Error::AddressCountMismatch {
            declared: 6,
            actual: 5
        })
    );
    let mut e = observed();
    addresses(&mut e).entries[5].table_index = 0;
    for q in [
        ordinal,
        PeExportQuery::Ordinal(0),
        PeExportQuery::Ordinal(u32::MAX),
    ] {
        assert_eq!(
            query(&e, q),
            Err(Error::AddressIndexMismatch {
                entry_index: 5,
                table_index: 0
            })
        );
    }
    addresses(&mut e).entries[5].table_index = 5;
    addresses(&mut e).entries[5].ordinal = 0;
    assert_eq!(
        query(&e, ordinal),
        Err(Error::OrdinalMismatch {
            entry_index: 5,
            expected: 0xffff_fffe,
            actual: 0
        })
    );
    let mut e = observed();
    addresses(&mut e).directory.ordinal_base = u32::MAX;
    addresses(&mut e).entries[0].ordinal = u32::MAX;
    assert_eq!(
        query(&e, ordinal),
        Err(Error::OrdinalOverflow {
            ordinal_base: u32::MAX,
            entry_index: 1
        })
    );
}

#[test]
fn complete_name_structure_precedes_matching_with_deterministic_error_order() {
    let mut e = observed();
    names(&mut e).entries[3].address_index = 6;
    for q in [PeExportQuery::Name("Alpha"), PeExportQuery::Name("Missing")] {
        assert_eq!(
            query(&e, q),
            Err(Error::NameAddressIndexOutOfRange {
                entry_index: 3,
                address_index: 6,
                address_count: 6
            })
        );
    }
    assert_eq!(
        lookup_pe_export_evidence(&e, PeExportQuery::Name("Alpha"), limits(9, 0)),
        Err(Error::RowsExceeded { rows: 10, limit: 9 })
    );
    assert_eq!(
        lookup_pe_export_evidence(&e, PeExportQuery::Name("Alpha"), limits(10, 59)),
        Err(Error::TextExceeded {
            bytes: 60,
            limit: 59
        })
    );
    names(&mut e).entries[3].table_index = 1;
    assert_eq!(
        query(&e, PeExportQuery::Name("Alpha")),
        Err(Error::NameIndexMismatch {
            entry_index: 3,
            table_index: 1
        })
    );
    names(&mut e).addresses.directory.number_of_name_pointers = 5;
    assert_eq!(
        query(&e, PeExportQuery::Name("Alpha")),
        Err(Error::NameCountMismatch {
            declared: 5,
            actual: 4
        })
    );
    names(&mut e).addresses.entries[5].table_index = 0;
    assert_eq!(
        query(&e, PeExportQuery::Name("Alpha")),
        Err(Error::AddressIndexMismatch {
            entry_index: 5,
            table_index: 0
        })
    );
    names(&mut e).addresses.directory.address_table_entries = 99;
    assert_eq!(
        query(&e, PeExportQuery::Name("Alpha")),
        Err(Error::AddressCountMismatch {
            declared: 99,
            actual: 6
        })
    );
}

#[test]
fn repeated_aliases_preserve_ambiguity_order_and_borrow_evidence_text() {
    let mut e = observed();
    names(&mut e).entries[0].name = "Alias".to_owned();
    names(&mut e).entries[0].address_index = 2;
    let result = {
        let temporary = String::from("Alias");
        lookup_pe_export_evidence(&e, PeExportQuery::Name(&temporary), limits(10, 57)).unwrap()
    };
    let PeExportSelection::AmbiguousName { matches } = result else {
        panic!();
    };
    assert_eq!(
        matches
            .iter()
            .map(|n| (n.table_index, n.address_index, n.name))
            .collect::<Vec<_>>(),
        vec![(0, 2, "Alias"), (3, 2, "Alias")]
    );
    let table = e.names.as_ref().unwrap().as_ref().unwrap();
    assert!(std::ptr::eq(
        matches[0].name.as_ptr(),
        table.entries[0].name.as_ptr()
    ));
    assert!(std::ptr::eq(
        matches[1].name.as_ptr(),
        table.entries[3].name.as_ptr()
    ));
}

#[test]
fn opaque_unicode_empty_text_and_coordinates_are_preserved() {
    let mut e = observed();
    names(&mut e).entries[1].name = "Å".to_owned();
    assert_eq!(
        lookup_pe_export_evidence(&e, PeExportQuery::Name("Å"), limits(10, 56)),
        Err(Error::TextExceeded {
            bytes: 57,
            limit: 56
        })
    );
    {
        let table = names(&mut e);
        table.entries[1].name_rva = RelativeVirtualAddress::new(u32::MAX);
        table.entries[1].name_file_offset = FileOffset::new(u64::MAX);
        table.addresses.entries[1].entry_file_offset = FileOffset::new(u64::MAX);
    }
    let PeExportSelection::Selected {
        address,
        name: Some(name),
    } = lookup_pe_export_evidence(&e, PeExportQuery::Name("Å"), limits(10, 57)).unwrap()
    else {
        panic!();
    };
    assert_eq!(name.name, "Å");
    assert_eq!(name.name_rva, RelativeVirtualAddress::new(u32::MAX));
    assert_eq!(name.name_file_offset, FileOffset::new(u64::MAX));
    assert_eq!(address.entry_file_offset, FileOffset::new(u64::MAX));
    names(&mut e).entries[1].name.clear();
    assert!(matches!(
        lookup_pe_export_evidence(&e, PeExportQuery::Name(""), limits(10, 55)),
        Ok(PeExportSelection::Selected { .. })
    ));
    let mut e = observed();
    let PeOwnedExportTarget::Forwarder { text, .. } =
        &mut names(&mut e).addresses.entries[2].target
    else {
        panic!();
    };
    text.clear();
    let PeExportSelection::Selected { address, .. } =
        lookup_pe_export_evidence(&e, PeExportQuery::Name("Alias"), limits(10, 49)).unwrap()
    else {
        panic!();
    };
    assert_eq!(
        address.target,
        PeExportTarget::Forwarder {
            rva: RelativeVirtualAddress::new(0x1080),
            text: ""
        }
    );
}
