use ring3_core::{
    FileOffset, PeResourceDataEntry, PeResourceDataEntryError as Error, PeResourceDataReference,
    PeResourceDirectoryError, PeRvaError, RelativeVirtualAddress, parse_pe_resource_data_entries,
    parse_pe_resource_directories,
};

fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn fixed(plus: bool) -> usize {
    if plus { 112 } else { 96 }
}
fn directory(bytes: &mut [u8], plus: bool, rva: u32, size: u32) {
    put32(bytes, 152 + fixed(plus) + 16, rva);
    put32(bytes, 152 + fixed(plus) + 20, size);
}
fn section(bytes: &mut [u8], plus: bool, rva: u32, virtual_size: u32, raw_size: u32) {
    for (delta, value) in [(8, virtual_size), (12, rva), (16, raw_size), (20, 512)] {
        put32(bytes, 152 + fixed(plus) + 24 + delta, value);
    }
}
fn table(bytes: &mut [u8], offset: u32, targets: &[u32]) {
    let at = 512 + offset as usize;
    bytes[at..at + 16].fill(0);
    bytes[at + 14..at + 16].copy_from_slice(&u16::try_from(targets.len()).unwrap().to_le_bytes());
    for (index, &target) in targets.iter().enumerate() {
        put32(bytes, at + 16 + 8 * index, u32::MAX);
        put32(bytes, at + 20 + 8 * index, target);
    }
}
fn fixture(plus: bool, targets: &[u32], records: &[(u32, [u32; 4])], size: u32) -> Vec<u8> {
    let mut bytes = vec![0; 512 + size as usize];
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 60, 128);
    bytes[128..132].copy_from_slice(b"PE\0\0");
    bytes[134..136].copy_from_slice(&1_u16.to_le_bytes());
    bytes[148..150].copy_from_slice(&u16::try_from(fixed(plus) + 24).unwrap().to_le_bytes());
    bytes[152..154].copy_from_slice(&if plus { 0x20b_u16 } else { 0x10b }.to_le_bytes());
    put32(&mut bytes, 212, 512);
    put32(&mut bytes, 152 + fixed(plus) - 4, 3);
    directory(&mut bytes, plus, 4096, size);
    section(&mut bytes, plus, 4096, size, size);
    table(&mut bytes, 0, targets);
    for &(offset, fields) in records {
        for (i, value) in fields.into_iter().enumerate() {
            put32(&mut bytes, 512 + offset as usize + 4 * i, value);
        }
    }
    bytes
}
fn record(offset: u32, fields: [u32; 4]) -> PeResourceDataEntry {
    PeResourceDataEntry {
        data_entry_offset: offset,
        data_entry_rva: RelativeVirtualAddress::new(4096 + offset),
        data_entry_file_offset: FileOffset::new(512 + u64::from(offset)),
        payload_rva: RelativeVirtualAddress::new(fields[0]),
        payload_size: fields[1],
        code_page: fields[2],
        reserved: fields[3],
    }
}
fn reference(
    directory_index: u16,
    entry_index: u16,
    data_entry_index: u16,
) -> PeResourceDataReference {
    PeResourceDataReference {
        directory_index,
        entry_index,
        data_entry_index,
    }
}

#[test]
fn exact_owned_records_preserve_first_reference_order_and_raw_payload_fields() {
    for plus in [false, true] {
        let first = [u32::MAX, u32::MAX, 0x1234_5678, 0x90ab_cdef];
        let bytes = fixture(plus, &[64, 48, 64], &[(48, [0; 4]), (64, first)], 80);
        let before = bytes.clone();
        let parsed = parse_pe_resource_data_entries(&bytes).unwrap().unwrap();
        assert_eq!(
            Some(parsed.directory_graph.clone()),
            parse_pe_resource_directories(&bytes).unwrap()
        );
        assert_eq!(bytes, before);
        drop(bytes);
        assert_eq!(parsed.data_entries, [record(64, first), record(48, [0; 4])]);
        assert_eq!(
            parsed.references,
            [reference(0, 0, 0), reference(0, 1, 1), reference(0, 2, 0)]
        );
    }
}
#[test]
fn absent_and_present_graphs_without_leaves_are_distinct() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, &[], &[], 16);
        let parsed = parse_pe_resource_data_entries(&bytes).unwrap().unwrap();
        assert_eq!(parsed.directory_graph.directories.len(), 1);
        assert!(parsed.data_entries.is_empty());
        assert!(parsed.references.is_empty());
        directory(&mut bytes, plus, 0, 0);
        assert_eq!(parse_pe_resource_data_entries(&bytes), Ok(None));
        let mut bytes = fixture(plus, &[0x8000_0018], &[], 40);
        table(&mut bytes, 24, &[]);
        let parsed = parse_pe_resource_data_entries(&bytes).unwrap().unwrap();
        assert_eq!(parsed.directory_graph.directories.len(), 2);
        assert!(parsed.references.is_empty());
    }
}
#[test]
fn shared_directories_do_not_expand_paths_but_distinct_leaf_occurrences_are_retained() {
    for plus in [false, true] {
        for shared in [false, true] {
            let mut bytes = fixture(
                plus,
                &[0x8000_0020, if shared { 0x8000_0020 } else { 0x8000_0038 }],
                &[(80, [1, 2, 3, 4])],
                96,
            );
            table(&mut bytes, 32, if shared { &[80, 80] } else { &[80] });
            if !shared {
                table(&mut bytes, 56, &[80]);
            }
            let parsed = parse_pe_resource_data_entries(&bytes).unwrap().unwrap();
            assert_eq!(parsed.data_entries, [record(80, [1, 2, 3, 4])]);
            assert_eq!(
                parsed.references,
                if shared {
                    vec![reference(1, 0, 0), reference(1, 1, 0)]
                } else {
                    vec![reference(1, 0, 0), reference(2, 0, 0)]
                }
            );
        }
    }
}
#[test]
fn equal_bytes_at_distinct_offsets_keep_distinct_identities() {
    for plus in [false, true] {
        let bytes = fixture(plus, &[32, 48], &[(32, [0; 4]), (48, [0; 4])], 64);
        let parsed = parse_pe_resource_data_entries(&bytes).unwrap().unwrap();
        assert_eq!(
            parsed.data_entries,
            [record(32, [0; 4]), record(48, [0; 4])]
        );
        assert_eq!(parsed.references, [reference(0, 0, 0), reference(0, 1, 1)]);
    }
}
#[test]
fn zero_unaligned_and_overlapping_record_offsets_are_not_special_cased() {
    for plus in [false, true] {
        let bytes = fixture(
            plus,
            &[0],
            &[(0, [u32::MAX, u32::MAX, u32::MAX, 0x0001_0000])],
            24,
        );
        assert_eq!(
            parse_pe_resource_data_entries(&bytes)
                .unwrap()
                .unwrap()
                .data_entries,
            [record(0, [u32::MAX, u32::MAX, u32::MAX, 0x0001_0000])]
        );
        let bytes = fixture(
            plus,
            &[33, 37],
            &[(33, [1, 2, 3, 4]), (37, [2, 3, 4, 5])],
            53,
        );
        assert_eq!(
            parse_pe_resource_data_entries(&bytes)
                .unwrap()
                .unwrap()
                .data_entries,
            [record(33, [1, 2, 3, 4]), record(37, [2, 3, 4, 5])]
        );
    }
}
#[test]
fn graph_errors_precede_invalid_leaf_records_and_preserve_the_original_error() {
    for plus in [false, true] {
        let bytes = fixture(plus, &[0x7fff_ffff, 0x8000_0000], &[], 32);
        assert_eq!(
            parse_pe_resource_data_entries(&bytes),
            Err(Error::Graph(PeResourceDirectoryError::Cycle {
                remaining_directories: 1
            }))
        );
        let bytes = fixture(plus, &[0x7fff_ffff, 0xffff_ffff], &[], 32);
        assert_eq!(
            parse_pe_resource_data_entries(&bytes),
            Err(Error::Graph(
                parse_pe_resource_directories(&bytes).unwrap_err()
            ))
        );
        let bytes = vec![0; 64];
        assert_eq!(
            parse_pe_resource_data_entries(&bytes),
            Err(Error::Graph(
                parse_pe_resource_directories(&bytes).unwrap_err()
            ))
        );
    }
}
#[test]
fn declared_extent_errors_identify_the_first_leaf_reference() {
    for plus in [false, true] {
        for (offset, size) in [(24, 39), (0x7fff_ffff, 40)] {
            let mut bytes = fixture(plus, &[offset], &[], 40);
            directory(&mut bytes, plus, 4096, size);
            assert_eq!(
                parse_pe_resource_data_entries(&bytes),
                Err(Error::DataEntryOutsideResource {
                    directory_index: 0,
                    entry_index: 0,
                    offset,
                    length: 16,
                    directory_size: size
                })
            );
        }
        let mut bytes = fixture(plus, &[0x8000_0018], &[], 48);
        table(&mut bytes, 24, &[40]);
        assert_eq!(
            parse_pe_resource_data_entries(&bytes),
            Err(Error::DataEntryOutsideResource {
                directory_index: 1,
                entry_index: 0,
                offset: 40,
                length: 16,
                directory_size: 48
            })
        );
    }
}
#[test]
fn the_full_record_requires_conservative_physical_backing() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, &[24], &[], 40);
        section(&mut bytes, plus, 4096, 40, 32);
        bytes.truncate(544);
        assert_eq!(
            parse_pe_resource_data_entries(&bytes),
            Err(Error::DataEntryRange {
                directory_index: 0,
                entry_index: 0,
                offset: 24,
                start: RelativeVirtualAddress::new(4120),
                length: 16,
                cause: PeRvaError::NotFileBacked {
                    start: RelativeVirtualAddress::new(4120),
                    length: 16,
                    section_index: 0
                }
            })
        );
    }
}
#[test]
fn the_last_record_can_end_exactly_at_the_rva_domain_boundary() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, &[24], &[(24, [u32::MAX; 4])], 40);
        let start = u32::MAX - 39;
        directory(&mut bytes, plus, start, 40);
        section(&mut bytes, plus, start, 40, 40);
        let parsed = parse_pe_resource_data_entries(&bytes).unwrap().unwrap();
        assert_eq!(
            parsed.data_entries[0].data_entry_rva,
            RelativeVirtualAddress::new(u32::MAX - 15)
        );
        assert_eq!(parsed.data_entries[0].payload_size, u32::MAX);
    }
}
#[test]
fn graph_entry_budget_bounds_shared_and_distinct_record_sets() {
    for plus in [false, true] {
        for shared in [false, true] {
            let data_start = 16 * 2064;
            let total_size = data_start + 4081 * 16;
            let mut bytes = fixture(plus, &[], &[], total_size);
            let mut next_record = 0;
            for node in 0..16 {
                let mut targets = Vec::new();
                for entry in 0..256 {
                    if node == 0 && entry < 15 {
                        targets.push(0x8000_0000 | ((entry + 1) * 2064));
                    } else {
                        targets.push(data_start + if shared { 0 } else { 16 * next_record });
                        next_record += 1;
                    }
                }
                table(&mut bytes, node * 2064, &targets);
            }
            assert_eq!(next_record, 4081);
            let parsed = parse_pe_resource_data_entries(&bytes).unwrap().unwrap();
            assert_eq!(parsed.references.len(), 4081);
            assert_eq!(parsed.data_entries.len(), if shared { 1 } else { 4081 });
            assert_eq!(parsed.references[0], reference(0, 15, 0));
            assert_eq!(
                parsed.references[4080],
                reference(15, 255, if shared { 0 } else { 4080 })
            );
        }
    }
}

fn generated_resource_fixture_matches_data_records(variable: &str, plus: bool) {
    use ring3_core::PeKind;

    let path = std::env::var_os(variable).expect("explicit generated resource fixture path");
    let bytes = std::fs::read(&path).unwrap();
    let before = bytes.clone();
    assert_eq!(bytes.len(), 2048);
    let parsed = parse_pe_resource_data_entries(&bytes).unwrap().unwrap();
    assert_eq!(
        parsed.directory_graph.kind,
        if plus { PeKind::Pe32Plus } else { PeKind::Pe32 }
    );
    assert_eq!(
        Some(parsed.directory_graph),
        parse_pe_resource_directories(&bytes).unwrap()
    );
    assert_eq!(
        parsed.data_entries,
        [PeResourceDataEntry {
            data_entry_offset: 80,
            data_entry_rva: RelativeVirtualAddress::new(8272),
            data_entry_file_offset: FileOffset::new(1616),
            payload_rva: RelativeVirtualAddress::new(8296),
            payload_size: 4,
            code_page: 0,
            reserved: 0,
        }]
    );
    assert_eq!(parsed.references, [reference(2, 0, 0)]);
    assert_eq!(bytes, before);
    assert_eq!(std::fs::read(path).unwrap(), before);
}

#[test]
#[ignore = "requires an explicit generated resource fixture path"]
fn generated_pe32_resource_data_entries_match_linked_records() {
    generated_resource_fixture_matches_data_records("RING3_RESOURCE_PE32_FIXTURE", false);
}

#[test]
#[ignore = "requires an explicit generated resource fixture path"]
fn generated_pe32plus_resource_data_entries_match_linked_records() {
    generated_resource_fixture_matches_data_records("RING3_RESOURCE_PE32PLUS_FIXTURE", true);
}
