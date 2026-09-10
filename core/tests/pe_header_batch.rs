use ring3_core::{
    FileOffset, PeHeaderBatchError, PeHeaderBatchLimits, PeHeaderError, PeKind,
    parse_pe_header_prefix_batch,
};

fn limits(files: u64, file_bytes: u64, total_bytes: u64) -> PeHeaderBatchLimits {
    PeHeaderBatchLimits {
        max_files: files,
        max_file_bytes: file_bytes,
        max_total_bytes: total_bytes,
    }
}

fn prefix(machine: u16, magic: u16) -> Vec<u8> {
    let mut bytes = vec![0; 90];
    bytes[..2].copy_from_slice(b"MZ");
    bytes[60..64].copy_from_slice(&64_u32.to_le_bytes());
    bytes[64..68].copy_from_slice(b"PE\0\0");
    bytes[68..70].copy_from_slice(&machine.to_le_bytes());
    bytes[70..72].copy_from_slice(&7_u16.to_le_bytes());
    bytes[84..86].copy_from_slice(&2_u16.to_le_bytes());
    bytes[86..88].copy_from_slice(&0x4321_u16.to_le_bytes());
    bytes[88..90].copy_from_slice(&magic.to_le_bytes());
    bytes
}

#[test]
fn empty_batch_and_zero_length_inputs_obey_zero_limits() {
    let empty = parse_pe_header_prefix_batch(&[], limits(0, 0, 0)).unwrap();
    assert_eq!(empty.total_bytes, 0);
    assert!(empty.headers.is_empty());
    let batch = parse_pe_header_prefix_batch(&[b"", b""], limits(2, 0, 0)).unwrap();
    assert_eq!(batch.total_bytes, 0);
    assert_eq!(
        batch.headers,
        vec![
            Err(PeHeaderError::OutOfBounds {
                offset: FileOffset::new(0),
                needed: 2,
                available: 0,
            });
            2
        ]
    );
}

#[test]
fn exact_limits_accept_and_each_one_lower_limit_refuses() {
    let bytes = prefix(0x14c, 0x10b);
    let inputs = [bytes.as_slice(), bytes.as_slice()];
    let batch = parse_pe_header_prefix_batch(&inputs, limits(2, 90, 180)).unwrap();
    assert_eq!(batch.total_bytes, 180);
    assert_eq!(batch.headers.len(), 2);
    assert!(batch.headers.iter().all(Result::is_ok));
    assert_eq!(
        parse_pe_header_prefix_batch(&inputs, limits(1, 89, 179)),
        Err(PeHeaderBatchError::FileCountExceeded { count: 2, limit: 1 })
    );
    assert_eq!(
        parse_pe_header_prefix_batch(&inputs, limits(2, 89, 179)),
        Err(PeHeaderBatchError::FileSizeExceeded {
            index: 0,
            size: 90,
            limit: 89
        })
    );
    assert_eq!(
        parse_pe_header_prefix_batch(&inputs, limits(2, 90, 179)),
        Err(PeHeaderBatchError::TotalSizeExceeded {
            index: 1,
            total: 180,
            limit: 179
        })
    );
}

#[test]
fn earlier_total_failure_precedes_a_later_file_failure() {
    assert_eq!(
        parse_pe_header_prefix_batch(&[b"abc", b"oversized"], limits(2, 3, 2)),
        Err(PeHeaderBatchError::TotalSizeExceeded {
            index: 0,
            total: 3,
            limit: 2
        })
    );
}

#[test]
fn a_late_budget_failure_precedes_an_early_malformed_prefix() {
    assert_eq!(
        parse_pe_header_prefix_batch(&[b"", b"abcd"], limits(2, 3, 3)),
        Err(PeHeaderBatchError::FileSizeExceeded {
            index: 1,
            size: 4,
            limit: 3
        })
    );
}

#[test]
fn repeated_references_charge_each_occurrence() {
    let bytes = prefix(0x8664, 0x20b);
    assert_eq!(
        parse_pe_header_prefix_batch(&[&bytes, &bytes], limits(2, 90, 90)),
        Err(PeHeaderBatchError::TotalSizeExceeded {
            index: 1,
            total: 180,
            limit: 90
        })
    );
}

#[test]
fn ordered_mixed_results_survive_dropped_inputs_without_changing_bytes() {
    let batch = {
        let owned = [prefix(0x14c, 0x10b), vec![], prefix(0x8664, 0x20b)];
        let before = owned.clone();
        let inputs: Vec<&[u8]> = owned.iter().map(Vec::as_slice).collect();
        let first = parse_pe_header_prefix_batch(&inputs, limits(3, 90, 180)).unwrap();
        assert_eq!(
            first,
            parse_pe_header_prefix_batch(&inputs, limits(3, 90, 180)).unwrap()
        );
        assert_eq!(owned, before);
        first
    };
    assert_eq!(batch.total_bytes, 180);
    assert_eq!(batch.headers.len(), 3);
    assert_eq!(batch.headers[0].unwrap().kind, PeKind::Pe32);
    assert_eq!(
        batch.headers[1],
        Err(PeHeaderError::OutOfBounds {
            offset: FileOffset::new(0),
            needed: 2,
            available: 0,
        })
    );
    assert_eq!(batch.headers[2].unwrap().kind, PeKind::Pe32Plus);
}

#[test]
fn unclassified_machine_and_raw_prefix_fields_are_preserved() {
    let bytes = prefix(0x7777, 0x20b);
    let batch = parse_pe_header_prefix_batch(&[&bytes], limits(1, 90, 90)).unwrap();
    let header = batch.headers[0].unwrap();
    assert_eq!(header.pe_offset, FileOffset::new(64));
    assert_eq!(header.machine, 0x7777);
    assert_eq!(header.number_of_sections, 7);
    assert_eq!(header.size_of_optional_header, 2);
    assert_eq!(header.characteristics, 0x4321);
    assert_eq!(header.kind, PeKind::Pe32Plus);
}
