use ring3_core::{
    FileOffset, PeKind, PeResourceDirectoryError as Error, PeResourceRootError, PeRvaError,
    RelativeVirtualAddress, parse_pe_resource_directories,
};

#[derive(Clone, Copy)]
enum Target {
    Child(usize),
    Leaf(u32),
    Offset(u32),
}
fn child(index: usize) -> (u32, Target) {
    (u32::MAX, Target::Child(index))
}
fn leaf(value: u32) -> (u32, Target) {
    (u32::MAX, Target::Leaf(value))
}
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
fn fixture(plus: bool, rows: &[Vec<(u32, Target)>]) -> (Vec<u8>, Vec<u32>) {
    let mut offsets = Vec::new();
    let mut size = 0;
    for row in rows {
        offsets.push(size);
        size += 16 + 8 * u32::try_from(row.len()).unwrap();
    }
    let mut bytes = vec![0; 512 + usize::try_from(size).unwrap()];
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 60, 128);
    bytes[128..132].copy_from_slice(b"PE\0\0");
    bytes[134..136].copy_from_slice(&1_u16.to_le_bytes());
    bytes[148..150].copy_from_slice(&u16::try_from(fixed(plus) + 24).unwrap().to_le_bytes());
    bytes[152..154].copy_from_slice(&if plus { 0x20b_u16 } else { 0x10b }.to_le_bytes());
    put32(&mut bytes, 212, 512);
    put32(&mut bytes, 152 + fixed(plus) - 4, 3);
    directory(&mut bytes, plus, 0x1000, size);
    section(&mut bytes, plus, 0x1000, size, size);
    for (i, row) in rows.iter().enumerate() {
        let at = 512 + offsets[i] as usize;
        put32(&mut bytes, at, 0x1234_5678);
        put32(&mut bytes, at + 4, 0x90ab_cdef);
        bytes[at + 8..at + 12].copy_from_slice(&[0x34, 0x12, 0xcd, 0xab]);
        bytes[at + 14..at + 16].copy_from_slice(&u16::try_from(row.len()).unwrap().to_le_bytes());
        for (j, &(name, target)) in row.iter().enumerate() {
            put32(&mut bytes, at + 16 + 8 * j, name);
            put32(
                &mut bytes,
                at + 20 + 8 * j,
                match target {
                    Target::Child(n) => 0x8000_0000 | offsets[n],
                    Target::Leaf(word) => word,
                    Target::Offset(offset) => 0x8000_0000 | offset,
                },
            );
        }
    }
    (bytes, offsets)
}

#[test]
fn raw_fields_coordinates_and_shared_edges_are_owned_and_ordered() {
    for plus in [false, true] {
        let (bytes, offsets) = fixture(
            plus,
            &[
                vec![child(1), child(1)],
                vec![child(2)],
                vec![leaf(0x7fff_ffff)],
            ],
        );
        let before = bytes.clone();
        let graph = parse_pe_resource_directories(&bytes).unwrap().unwrap();
        assert_eq!(bytes, before);
        drop(bytes);
        assert_eq!(
            graph.kind,
            if plus { PeKind::Pe32Plus } else { PeKind::Pe32 }
        );
        assert_eq!(graph.directory_rva, RelativeVirtualAddress::new(4096));
        assert_eq!(graph.directory_file_offset, FileOffset::new(512));
        assert_eq!(graph.directory_size, 80);
        assert_eq!(graph.directories.len(), 3);
        for (i, node) in graph.directories.iter().enumerate() {
            assert_eq!(node.directory_offset, offsets[i]);
            assert_eq!(
                node.directory_rva,
                RelativeVirtualAddress::new(4096 + offsets[i])
            );
            assert_eq!(
                node.directory_file_offset,
                FileOffset::new(512 + u64::from(offsets[i]))
            );
            assert_eq!(
                (
                    node.characteristics,
                    node.time_date_stamp,
                    node.major_version,
                    node.minor_version
                ),
                (0x1234_5678, 0x90ab_cdef, 0x1234, 0xabcd)
            );
            assert_eq!(node.number_of_named_entries, 0);
            assert_eq!(usize::from(node.number_of_id_entries), node.entries.len());
            assert_eq!(usize::from(node.longest_root_path), i);
            for (j, e) in node.entries.iter().enumerate() {
                let displacement = offsets[i] + 16 + 8 * u32::try_from(j).unwrap();
                assert_eq!(
                    e.entry_rva,
                    RelativeVirtualAddress::new(4096 + displacement)
                );
                assert_eq!(
                    e.entry_file_offset,
                    FileOffset::new(512 + u64::from(displacement))
                );
                assert_eq!(e.raw_name_or_id, u32::MAX);
            }
        }
        assert_eq!(
            graph.directories[0]
                .entries
                .iter()
                .map(|e| e.child_directory_index)
                .collect::<Vec<_>>(),
            [Some(1), Some(1)]
        );
        assert_eq!(
            graph.directories[1].entries[0].child_directory_index,
            Some(2)
        );
        assert_eq!(graph.directories[2].entries[0].child_directory_index, None);
        assert_eq!(
            graph.directories[2].entries[0].raw_data_or_subdirectory,
            0x7fff_ffff
        );
    }
}
#[test]
fn absence_empty_and_base_root_error_precedence_are_preserved() {
    for plus in [false, true] {
        let (mut bytes, _) = fixture(plus, &[vec![]]);
        assert_eq!(
            parse_pe_resource_directories(&bytes)
                .unwrap()
                .unwrap()
                .directories
                .len(),
            1
        );
        directory(&mut bytes, plus, 0, 0);
        assert_eq!(parse_pe_resource_directories(&bytes), Ok(None));
        directory(&mut bytes, plus, 4096, 0);
        assert!(matches!(
            parse_pe_resource_directories(&bytes),
            Err(Error::Root(
                PeResourceRootError::InconsistentDirectory { .. }
            ))
        ));
        directory(&mut bytes, plus, 0, 0);
        bytes.pop();
        assert!(matches!(
            parse_pe_resource_directories(&bytes),
            Err(Error::Root(PeResourceRootError::Base(PeRvaError::Parse(_))))
        ));
    }
}
#[test]
fn diamond_and_shallow_first_aliases_keep_the_longest_root_path() {
    for plus in [false, true] {
        for (rows, depths) in [
            (
                vec![
                    vec![child(1), child(2)],
                    vec![child(3)],
                    vec![child(3)],
                    vec![],
                ],
                vec![0, 1, 1, 2],
            ),
            (
                vec![
                    vec![child(3), child(1)],
                    vec![child(2)],
                    vec![child(3)],
                    vec![],
                ],
                vec![0, 3, 1, 2],
            ),
        ] {
            let (bytes, _) = fixture(plus, &rows);
            let graph = parse_pe_resource_directories(&bytes).unwrap().unwrap();
            assert_eq!(
                graph
                    .directories
                    .iter()
                    .map(|n| n.longest_root_path)
                    .collect::<Vec<_>>(),
                depths
            );
        }
    }
}
#[test]
fn cycles_are_rejected_before_depth_and_report_the_whole_blocked_residue() {
    for plus in [false, true] {
        for (rows, remaining) in [
            (vec![vec![child(0)]], 1),
            (vec![vec![child(1)], vec![child(0)]], 2),
            (vec![vec![child(0), child(1)], vec![]], 2),
        ] {
            let (bytes, _) = fixture(plus, &rows);
            assert_eq!(
                parse_pe_resource_directories(&bytes),
                Err(Error::Cycle {
                    remaining_directories: remaining
                })
            );
        }
        let mut rows = (0..17).map(|i| vec![child(i + 1)]).collect::<Vec<_>>();
        rows.push(vec![child(17)]);
        let (bytes, _) = fixture(plus, &rows);
        assert_eq!(
            parse_pe_resource_directories(&bytes),
            Err(Error::Cycle {
                remaining_directories: 1
            })
        );
    }
}
#[test]
fn unique_directory_limit_deduplicates_before_refusing_a_new_unread_target() {
    for plus in [false, true] {
        let mut rows = vec![vec![]; 256];
        rows[0] = (1..256).map(child).collect();
        let (bytes, _) = fixture(plus, &rows);
        assert_eq!(
            parse_pe_resource_directories(&bytes)
                .unwrap()
                .unwrap()
                .directories
                .len(),
            256
        );
        rows[0].push(child(1));
        let (bytes, _) = fixture(plus, &rows);
        assert_eq!(
            parse_pe_resource_directories(&bytes)
                .unwrap()
                .unwrap()
                .directories
                .len(),
            256
        );
        rows[0][255] = (0, Target::Offset(0x7fff_ffff));
        let (bytes, _) = fixture(plus, &rows);
        assert_eq!(
            parse_pe_resource_directories(&bytes),
            Err(Error::DirectoryLimitExceeded {
                offset: 0x7fff_ffff,
                count: 257,
                limit: 256
            })
        );
    }
}
#[test]
fn per_directory_counts_are_widened_and_bounded_before_full_entries() {
    for plus in [false, true] {
        let (bytes, _) = fixture(plus, &[vec![leaf(0); 256]]);
        assert_eq!(
            parse_pe_resource_directories(&bytes)
                .unwrap()
                .unwrap()
                .directories[0]
                .entries
                .len(),
            256
        );
        let (bytes, _) = fixture(plus, &[vec![leaf(0); 257]]);
        assert_eq!(
            parse_pe_resource_directories(&bytes),
            Err(Error::Root(PeResourceRootError::EntryLimitExceeded {
                count: 257,
                limit: 256
            }))
        );
        let (mut bytes, offsets) = fixture(plus, &[vec![child(1)], vec![leaf(0); 257]]);
        assert_eq!(
            parse_pe_resource_directories(&bytes),
            Err(Error::EntryLimitExceeded {
                directory_offset: offsets[1],
                count: 257,
                limit: 256
            })
        );
        let at = 512 + offsets[1] as usize;
        put32(&mut bytes, at + 12, u32::MAX);
        assert_eq!(
            parse_pe_resource_directories(&bytes),
            Err(Error::EntryLimitExceeded {
                directory_offset: offsets[1],
                count: 131_070,
                limit: 256
            })
        );
    }
}
#[test]
fn total_entry_limit_counts_duplicate_edges_and_opaque_leaves_without_path_expansion() {
    for plus in [false, true] {
        let mut rows = (0..16).map(|i| vec![child(i + 1); 256]).collect::<Vec<_>>();
        rows.push(vec![]);
        let (bytes, _) = fixture(plus, &rows);
        let graph = parse_pe_resource_directories(&bytes).unwrap().unwrap();
        assert_eq!(graph.directories.len(), 17);
        assert_eq!(
            graph
                .directories
                .iter()
                .map(|n| n.entries.len())
                .sum::<usize>(),
            4096
        );
        assert_eq!(graph.directories[16].longest_root_path, 16);
        rows[16].push(leaf(0x7fff_ffff));
        let (mut bytes, offsets) = fixture(plus, &rows);
        directory(&mut bytes, plus, 4096, offsets[16] + 16);
        assert_eq!(
            parse_pe_resource_directories(&bytes),
            Err(Error::TotalEntryLimitExceeded {
                directory_offset: offsets[16],
                count: 4097,
                limit: 4096
            })
        );
    }
}
#[test]
fn depth_limit_catches_a_deep_alternate_path_to_an_already_seen_node() {
    for plus in [false, true] {
        for depth in [16, 17] {
            let mut rows = (0..depth).map(|i| vec![child(i + 1)]).collect::<Vec<_>>();
            rows.push(vec![]);
            for alias in [false, true] {
                if alias {
                    rows[0].insert(0, child(depth));
                }
                let (bytes, offsets) = fixture(plus, &rows);
                let result = parse_pe_resource_directories(&bytes);
                if depth == 16 {
                    assert_eq!(
                        result
                            .unwrap()
                            .unwrap()
                            .directories
                            .iter()
                            .map(|n| n.longest_root_path)
                            .max(),
                        Some(16)
                    );
                } else {
                    assert_eq!(
                        result,
                        Err(Error::DepthLimitExceeded {
                            directory_offset: offsets[17],
                            depth: 17,
                            limit: 16
                        })
                    );
                }
            }
        }
    }
}
#[test]
fn child_declared_extents_precede_physical_reads() {
    for plus in [false, true] {
        for offset in [24, 0x7fff_ffff] {
            let (bytes, _) = fixture(plus, &[vec![(0, Target::Offset(offset))]]);
            assert_eq!(
                parse_pe_resource_directories(&bytes),
                Err(Error::DirectoryOutsideResource {
                    offset,
                    length: 16,
                    directory_size: 24
                })
            );
        }
        let (mut bytes, offsets) = fixture(plus, &[vec![child(1)], vec![leaf(0)]]);
        directory(&mut bytes, plus, 4096, 40);
        assert_eq!(
            parse_pe_resource_directories(&bytes),
            Err(Error::DirectoryOutsideResource {
                offset: offsets[1],
                length: 24,
                directory_size: 40
            })
        );
    }
}
#[test]
fn child_header_and_full_prefix_require_conservative_backing() {
    for plus in [false, true] {
        for (raw, required) in [(32, 16), (40, 24)] {
            let (mut bytes, offsets) = fixture(plus, &[vec![child(1)], vec![leaf(0)]]);
            section(&mut bytes, plus, 4096, 48, raw);
            bytes.truncate(512 + raw as usize);
            assert_eq!(
                parse_pe_resource_directories(&bytes),
                Err(Error::DirectoryRange {
                    offset: offsets[1],
                    start: RelativeVirtualAddress::new(4120),
                    length: required,
                    cause: PeRvaError::NotFileBacked {
                        start: RelativeVirtualAddress::new(4120),
                        length: required,
                        section_index: 0
                    }
                })
            );
        }
    }
}
#[test]
fn unaligned_overlapping_tables_and_invalid_names_or_leaf_words_remain_raw() {
    for plus in [false, true] {
        let (mut bytes, _) = fixture(plus, &[vec![(0, Target::Offset(4))]]);
        let graph = parse_pe_resource_directories(&bytes).unwrap().unwrap();
        assert_eq!(
            graph.directories[1].directory_file_offset,
            FileOffset::new(516)
        );
        assert!(graph.directories[1].entries.is_empty());
        put32(&mut bytes, 528, u32::MAX);
        put32(&mut bytes, 532, 0x7fff_ffff);
        bytes[524..528].copy_from_slice(&[1, 0, 0, 0]);
        assert_eq!(
            parse_pe_resource_directories(&bytes)
                .unwrap()
                .unwrap()
                .directories
                .len(),
            1
        );
        let (mut bytes, _) = fixture(plus, &[vec![child(1)], vec![]]);
        bytes.insert(536, 0);
        directory(&mut bytes, plus, 4096, 41);
        section(&mut bytes, plus, 4096, 41, 41);
        put32(&mut bytes, 532, 0x8000_0019);
        let graph = parse_pe_resource_directories(&bytes).unwrap().unwrap();
        assert_eq!(graph.directories[1].directory_offset, 25);
        assert_eq!(
            graph.directories[1].directory_file_offset,
            FileOffset::new(537)
        );
    }
}
#[test]
fn exact_rva_domain_end_is_supported_without_wrapping_coordinates() {
    for plus in [false, true] {
        let (mut bytes, _) = fixture(plus, &[vec![child(1)], vec![leaf(0)]]);
        let start = u32::MAX - 47;
        directory(&mut bytes, plus, start, 48);
        section(&mut bytes, plus, start, 48, 48);
        let graph = parse_pe_resource_directories(&bytes).unwrap().unwrap();
        assert_eq!(
            graph.directories[1].entries[0].entry_rva,
            RelativeVirtualAddress::new(u32::MAX - 7)
        );
    }
}
