use ring3_core::{
    FileOffset, PeExportAddressError, PeExportBatchError, PeExportBatchLimits, PeExportLookupError,
    PeExportNameError, PeExportQuery, PeExportSelection, PeExportTarget, RelativeVirtualAddress,
    lookup_pe_export_batch,
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

fn limits(queries: u64, rows: u64) -> PeExportBatchLimits {
    PeExportBatchLimits {
        max_queries: queries,
        max_selection_rows: rows,
    }
}

#[test]
fn count_refusal_precedes_input_and_empty_queries_do_not_parse() {
    assert_eq!(
        lookup_pe_export_batch(b"", &[PeExportQuery::Name("entry")], limits(0, 0)),
        Err(PeExportBatchError::QueryCountExceeded { count: 1, limit: 0 })
    );
    let batch = lookup_pe_export_batch(b"", &[], limits(0, 0)).unwrap();
    assert_eq!(batch.selection_rows, 0);
    assert!(batch.selections.is_empty());
}

#[test]
fn both_widths_budget_duplicate_rows_and_keep_query_order() {
    for plus in [false, true] {
        let bytes = fixture(plus, 2);
        let queries = [
            PeExportQuery::Name("zeta"),
            PeExportQuery::Name("absent"),
            PeExportQuery::Ordinal(8),
            PeExportQuery::Ordinal(6),
        ];
        let batch = lookup_pe_export_batch(&bytes, &queries, limits(4, 3)).unwrap();
        assert_eq!(batch.selection_rows, 3);
        let PeExportSelection::AmbiguousName { matches } = batch.selections[0].as_ref().unwrap()
        else {
            panic!()
        };
        assert_eq!(matches.len(), 2);
        assert_eq!((matches[0].table_index, matches[1].table_index), (0, 1));
        assert_eq!(batch.selections[1], Ok(PeExportSelection::NameNotFound));
        assert!(matches!(
            batch.selections[2],
            Ok(PeExportSelection::Selected { name: None, .. })
        ));
        assert_eq!(
            batch.selections[3],
            Ok(PeExportSelection::OrdinalBeforeBase {
                ordinal: 6,
                base: 7
            })
        );
        assert_eq!(
            lookup_pe_export_batch(&bytes, &queries, limits(4, 2)),
            Err(PeExportBatchError::SelectionRowsExceeded {
                index: 2,
                total: 3,
                limit: 2
            })
        );
        assert_eq!(
            lookup_pe_export_batch(&bytes, &queries, limits(3, 3)),
            Err(PeExportBatchError::QueryCountExceeded { count: 4, limit: 3 })
        );
    }
}

#[test]
fn name_parse_errors_charge_zero_and_do_not_block_ordinals() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 2);
        row(&mut bytes, 1, 0xc010, 3);
        let queries = [PeExportQuery::Name("zeta"), PeExportQuery::Ordinal(8)];
        let batch = lookup_pe_export_batch(&bytes, &queries, limits(2, 1)).unwrap();
        assert_eq!(batch.selection_rows, 1);
        assert_eq!(
            batch.selections[0],
            Err(PeExportLookupError::Names(
                PeExportNameError::AddressIndexOutOfRange {
                    entry_index: 1,
                    address_index: 3,
                    address_count: 3
                }
            ))
        );
        assert!(matches!(
            batch.selections[1],
            Ok(PeExportSelection::Selected { .. })
        ));
        assert_eq!(
            lookup_pe_export_batch(&bytes, &queries, limits(2, 0)),
            Err(PeExportBatchError::SelectionRowsExceeded {
                index: 1,
                total: 1,
                limit: 0
            })
        );
        assert_eq!(
            lookup_pe_export_batch(&bytes, &[queries[1], queries[0]], limits(2, 0)),
            Err(PeExportBatchError::SelectionRowsExceeded {
                index: 0,
                total: 1,
                limit: 0
            })
        );
    }
}

#[test]
fn absence_and_address_errors_are_zero_row_outcomes() {
    let mut bytes = fixture(false, 1);
    let queries = [PeExportQuery::Name("zeta"), PeExportQuery::Ordinal(8)];
    directory(&mut bytes, false, 0, 0);
    let absent = lookup_pe_export_batch(&bytes, &queries, limits(2, 0)).unwrap();
    assert_eq!(absent.selection_rows, 0);
    assert_eq!(
        absent.selections,
        vec![Ok(PeExportSelection::DirectoryAbsent); 2]
    );
    directory(&mut bytes, false, 0x1000, 0x1000);
    put32(&mut bytes, 528, u32::MAX);
    let errors = lookup_pe_export_batch(&bytes, &queries, limits(2, 0)).unwrap();
    assert_eq!(errors.selection_rows, 0);
    let cause = PeExportAddressError::OrdinalOverflow {
        ordinal_base: u32::MAX,
        entry_index: 1,
    };
    assert_eq!(
        errors.selections,
        vec![
            Err(PeExportLookupError::Names(PeExportNameError::Addresses(
                cause
            ))),
            Err(PeExportLookupError::Addresses(cause))
        ]
    );
}

#[test]
fn selected_empty_and_forwarder_targets_each_cost_one_row() {
    let bytes = fixture(false, 0);
    let queries = [PeExportQuery::Ordinal(7), PeExportQuery::Ordinal(9)];
    let batch = lookup_pe_export_batch(&bytes, &queries, limits(2, 2)).unwrap();
    assert_eq!(batch.selection_rows, 2);
    let PeExportSelection::Selected {
        address,
        name: None,
    } = batch.selections[0].as_ref().unwrap()
    else {
        panic!()
    };
    assert_eq!(address.target, PeExportTarget::Empty);
    let PeExportSelection::Selected {
        address,
        name: None,
    } = batch.selections[1].as_ref().unwrap()
    else {
        panic!()
    };
    assert_eq!(address.entry_file_offset, FileOffset::new(8712));
    assert_eq!(
        address.target,
        PeExportTarget::Forwarder {
            rva: RelativeVirtualAddress::new(0x1100),
            text: "W.F"
        }
    );
    assert_eq!(
        lookup_pe_export_batch(&bytes, &queries, limits(2, 1)),
        Err(PeExportBatchError::SelectionRowsExceeded {
            index: 1,
            total: 2,
            limit: 1
        })
    );
}

#[test]
fn batch_results_outlive_queries_and_preserve_input_borrows() {
    let bytes = fixture(true, 2);
    let before = bytes.clone();
    let batch = {
        let name = String::from("zeta");
        let queries = [PeExportQuery::Name(&name), PeExportQuery::Ordinal(9)];
        lookup_pe_export_batch(&bytes, &queries, limits(2, 3)).unwrap()
    };
    let PeExportSelection::AmbiguousName { matches } = batch.selections[0].as_ref().unwrap() else {
        panic!()
    };
    assert!(
        matches
            .iter()
            .all(|row| std::ptr::eq(row.name.as_ptr(), bytes[file_offset(0xc000)..].as_ptr()))
    );
    let PeExportSelection::Selected { address, .. } = batch.selections[1].as_ref().unwrap() else {
        panic!()
    };
    let PeExportTarget::Forwarder { text, .. } = address.target else {
        panic!()
    };
    assert!(std::ptr::eq(text.as_ptr(), bytes[768..].as_ptr()));
    assert_eq!(bytes, before);
}

#[test]
fn repeated_budget_refusals_and_successes_preserve_inputs() {
    let bytes = fixture(false, 2);
    let before = bytes.clone();
    let queries = [PeExportQuery::Name("zeta"); 2];
    let expected = lookup_pe_export_batch(&bytes, &queries, limits(2, 4)).unwrap();
    assert_eq!(expected.selection_rows, 4);
    for _ in 0..16 {
        assert_eq!(
            lookup_pe_export_batch(&bytes, &queries, limits(2, 3)),
            Err(PeExportBatchError::SelectionRowsExceeded {
                index: 1,
                total: 4,
                limit: 3
            })
        );
        assert_eq!(
            lookup_pe_export_batch(&bytes, &queries, limits(2, 4)),
            Ok(expected.clone())
        );
    }
    assert_eq!(bytes, before);
}
