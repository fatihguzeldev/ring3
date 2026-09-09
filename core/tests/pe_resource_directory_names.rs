use ring3_core::{
    FileOffset, PeResourceDirectoryError, PeResourceDirectoryNameError as Error, PeRvaError,
    RelativeVirtualAddress, parse_pe_resource_directory_names, parse_pe_resource_root_names,
};

fn put16(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
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
fn section(bytes: &mut [u8], plus: bool, rva: u32, size: u32, raw: u32) {
    for (delta, value) in [(8, size), (12, rva), (16, raw), (20, 512)] {
        put32(bytes, 152 + fixed(plus) + 24 + delta, value);
    }
}
fn fixture(plus: bool, size: u32) -> Vec<u8> {
    let mut bytes = vec![0; 512 + size as usize];
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 60, 128);
    bytes[128..132].copy_from_slice(b"PE\0\0");
    put16(&mut bytes, 134, 1);
    put16(&mut bytes, 148, u16::try_from(fixed(plus) + 24).unwrap());
    put16(&mut bytes, 152, if plus { 0x20b } else { 0x10b });
    put32(&mut bytes, 212, 512);
    put32(&mut bytes, 152 + fixed(plus) - 4, 3);
    directory(&mut bytes, plus, 4096, size);
    section(&mut bytes, plus, 4096, size, size);
    bytes
}
fn table(bytes: &mut [u8], offset: u32, named: u16, entries: &[(u32, u32)]) {
    let at = 512 + offset as usize;
    bytes[at..at + 16].fill(0);
    put16(bytes, at + 12, named);
    put16(
        bytes,
        at + 14,
        u16::try_from(entries.len()).unwrap() - named,
    );
    for (index, &(name, target)) in entries.iter().enumerate() {
        put32(bytes, at + 16 + 8 * index, name);
        put32(bytes, at + 20 + 8 * index, target);
    }
}
fn name(bytes: &mut [u8], offset: u32, units: &[u16]) {
    let at = 512 + offset as usize;
    put16(bytes, at, u16::try_from(units.len()).unwrap());
    for (i, &unit) in units.iter().enumerate() {
        put16(bytes, at + 2 + 2 * i, unit);
    }
}
fn named(offset: u32) -> (u32, u32) {
    (0x8000_0000 | offset, 0x7fff_ffff)
}

#[test]
fn nested_names_preserve_raw_bytes_coordinates_order_and_borrowed_aliases() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 256);
        table(
            &mut bytes,
            0,
            2,
            &[(0x8000_0080, 0x8000_0040), named(144), (u32::MAX, 0)],
        );
        table(&mut bytes, 64, 1, &[named(128), (u32::MAX, 0)]);
        name(&mut bytes, 128, &[65, 0, 0xd800]);
        name(&mut bytes, 144, &[0xdc00]);
        let before = bytes.clone();
        let parsed = parse_pe_resource_directory_names(&bytes).unwrap().unwrap();
        assert_eq!(parsed.names.len(), 3);
        assert_eq!(
            parsed
                .names
                .iter()
                .map(|n| (n.directory_index, n.entry_index, n.name_offset))
                .collect::<Vec<_>>(),
            [(0, 0, 128), (0, 1, 144), (1, 0, 128)]
        );
        for n in &parsed.names {
            assert_eq!(
                n.name_rva,
                RelativeVirtualAddress::new(4096 + n.name_offset)
            );
            assert_eq!(
                n.name_file_offset,
                FileOffset::new(512 + u64::from(n.name_offset))
            );
            assert_eq!(n.utf16le.len(), 2 * usize::from(n.code_unit_count));
            assert_eq!(
                n.utf16le.as_ptr(),
                bytes[514 + n.name_offset as usize..].as_ptr()
            );
        }
        assert_eq!(parsed.names[0].utf16le, &[65, 0, 0, 0, 0, 0xd8]);
        assert_eq!(parsed.names[1].utf16le, &[0, 0xdc]);
        assert_eq!(
            parsed.names[0].utf16le.as_ptr(),
            parsed.names[2].utf16le.as_ptr()
        );
        let root = parse_pe_resource_root_names(&bytes).unwrap().unwrap();
        for (old, new) in root.names.iter().zip(&parsed.names) {
            assert_eq!(
                (
                    old.entry_index,
                    old.name_offset,
                    old.name_rva,
                    old.name_file_offset,
                    old.code_unit_count,
                    old.utf16le
                ),
                (
                    new.entry_index,
                    new.name_offset,
                    new.name_rva,
                    new.name_file_offset,
                    new.code_unit_count,
                    new.utf16le
                )
            );
        }
        assert_eq!(bytes, before);
    }
}
#[test]
fn absence_empty_names_and_raw_id_words_remain_distinct() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 256);
        table(&mut bytes, 0, 0, &[(u32::MAX, 0)]);
        assert!(
            parse_pe_resource_directory_names(&bytes)
                .unwrap()
                .unwrap()
                .names
                .is_empty()
        );
        table(&mut bytes, 0, 1, &[named(128)]);
        name(&mut bytes, 128, &[]);
        let parsed = parse_pe_resource_directory_names(&bytes).unwrap().unwrap();
        assert_eq!(parsed.names.len(), 1);
        assert_eq!(parsed.names[0].code_unit_count, 0);
        assert!(parsed.names[0].utf16le.is_empty());
        directory(&mut bytes, plus, 0, 0);
        assert_eq!(parse_pe_resource_directory_names(&bytes), Ok(None));
    }
}
#[test]
fn shared_directory_paths_do_not_repeat_its_named_rows() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 256);
        table(&mut bytes, 0, 0, &[(0, 0x8000_0040), (0, 0x8000_0040)]);
        table(&mut bytes, 64, 2, &[named(128), named(128)]);
        name(&mut bytes, 128, &[65]);
        let parsed = parse_pe_resource_directory_names(&bytes).unwrap().unwrap();
        assert_eq!(
            parsed
                .names
                .iter()
                .map(|n| (n.directory_index, n.entry_index))
                .collect::<Vec<_>>(),
            [(1, 0), (1, 1)]
        );
    }
}
#[test]
fn only_the_declared_named_prefix_requires_the_name_encoding_bit() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 256);
        table(&mut bytes, 0, 0, &[(0, 0x8000_0040)]);
        table(&mut bytes, 64, 1, &[(7, 0), (u32::MAX, 0)]);
        assert_eq!(
            parse_pe_resource_directory_names(&bytes),
            Err(Error::UnsupportedNameEncoding {
                directory_index: 1,
                entry_index: 0,
                raw_name_or_id: 7
            })
        );
        table(&mut bytes, 64, 0, &[(7, 0), (u32::MAX, 0)]);
        assert!(
            parse_pe_resource_directory_names(&bytes)
                .unwrap()
                .unwrap()
                .names
                .is_empty()
        );
    }
}
#[test]
fn per_name_limit_precedes_aggregate_and_full_record_reads() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 4096);
        table(&mut bytes, 0, 1, &[named(128)]);
        name(&mut bytes, 128, &vec![0; 1024]);
        assert_eq!(
            parse_pe_resource_directory_names(&bytes)
                .unwrap()
                .unwrap()
                .names[0]
                .code_unit_count,
            1024
        );
        put16(&mut bytes, 640, 1025);
        directory(&mut bytes, plus, 4096, 130);
        assert_eq!(
            parse_pe_resource_directory_names(&bytes),
            Err(Error::NameLengthLimitExceeded {
                directory_index: 0,
                entry_index: 0,
                code_unit_count: 1025,
                limit: 1024
            })
        );
    }
}
#[test]
fn aggregate_budget_spans_directories_and_counts_repeated_name_offsets() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 8192);
        let mut root = vec![named(4096); 16];
        root.push((0, 0x8000_0200));
        table(&mut bytes, 0, 16, &root);
        table(&mut bytes, 512, 16, &[named(4096); 16]);
        name(&mut bytes, 4096, &vec![65; 1024]);
        let parsed = parse_pe_resource_directory_names(&bytes).unwrap().unwrap();
        assert_eq!(parsed.names.len(), 32);
        assert_eq!(
            parsed
                .names
                .iter()
                .map(|n| u32::from(n.code_unit_count))
                .sum::<u32>(),
            32_768
        );
        let mut child = vec![named(4096); 16];
        child.push(named(6200));
        table(&mut bytes, 512, 17, &child);
        name(&mut bytes, 6200, &[0]);
        assert_eq!(
            parse_pe_resource_directory_names(&bytes),
            Err(Error::NameCodeUnitBudgetExceeded {
                directory_index: 1,
                entry_index: 16,
                used: 32_768,
                code_unit_count: 1,
                limit: 32_768
            })
        );
    }
}
#[test]
fn complete_graph_errors_precede_names_without_changing_root_only_reading() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 64);
        table(&mut bytes, 0, 1, &[(7, 0x8000_0000)]);
        assert_eq!(
            parse_pe_resource_directory_names(&bytes),
            Err(Error::Graph(PeResourceDirectoryError::Cycle {
                remaining_directories: 1
            }))
        );
        assert!(matches!(
            parse_pe_resource_root_names(&bytes),
            Err(ring3_core::PeResourceRootNameError::UnsupportedNameEncoding { .. })
        ));
        let bytes = vec![0; 64];
        assert!(matches!(
            parse_pe_resource_directory_names(&bytes),
            Err(Error::Graph(PeResourceDirectoryError::Root(_)))
        ));
    }
}
#[test]
fn relative_prefix_and_full_extents_keep_directory_entry_attribution() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 256);
        table(&mut bytes, 0, 0, &[(0, 0x8000_0020)]);
        table(&mut bytes, 32, 1, &[named(0x7fff_ffff)]);
        assert_eq!(
            parse_pe_resource_directory_names(&bytes),
            Err(Error::NameOutsideDirectory {
                directory_index: 1,
                entry_index: 0,
                offset: 0x7fff_ffff,
                length: 2,
                directory_size: 256
            })
        );
        table(&mut bytes, 32, 1, &[named(64)]);
        name(&mut bytes, 64, &[0; 8]);
        directory(&mut bytes, plus, 4096, 70);
        assert_eq!(
            parse_pe_resource_directory_names(&bytes),
            Err(Error::NameOutsideDirectory {
                directory_index: 1,
                entry_index: 0,
                offset: 64,
                length: 18,
                directory_size: 70
            })
        );
    }
}
#[test]
fn prefix_and_full_name_records_require_conservative_backing() {
    for plus in [false, true] {
        for (raw, length) in [(65, 2), (70, 18)] {
            let mut bytes = fixture(plus, 128);
            table(&mut bytes, 0, 1, &[named(64)]);
            name(&mut bytes, 64, &[0; 8]);
            section(&mut bytes, plus, 4096, 128, raw);
            bytes.truncate(512 + raw as usize);
            assert_eq!(
                parse_pe_resource_directory_names(&bytes),
                Err(Error::NameRange {
                    directory_index: 0,
                    entry_index: 0,
                    start: RelativeVirtualAddress::new(4160),
                    length,
                    cause: PeRvaError::NotFileBacked {
                        start: RelativeVirtualAddress::new(4160),
                        length,
                        section_index: 0
                    }
                })
            );
        }
    }
}
#[test]
fn zero_unaligned_and_overlapping_names_stay_undecoded() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 256);
        table(&mut bytes, 0, 1, &[named(0)]);
        let parsed = parse_pe_resource_directory_names(&bytes).unwrap().unwrap();
        assert_eq!(parsed.names[0].name_offset, 0);
        assert_eq!(parsed.names[0].code_unit_count, 0);
        table(&mut bytes, 0, 2, &[named(129), named(131)]);
        name(&mut bytes, 129, &[1, 99]);
        let parsed = parse_pe_resource_directory_names(&bytes).unwrap().unwrap();
        assert_eq!(parsed.names[0].utf16le, &[1, 0, 99, 0]);
        assert_eq!(parsed.names[1].utf16le, &[99, 0]);
    }
}
#[test]
fn a_name_record_can_end_exactly_at_the_rva_domain_boundary() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 256);
        table(&mut bytes, 0, 1, &[named(248)]);
        name(&mut bytes, 248, &[0, 0xd800, 0xdc00]);
        let start = u32::MAX - 255;
        directory(&mut bytes, plus, start, 256);
        section(&mut bytes, plus, start, 256, 256);
        let parsed = parse_pe_resource_directory_names(&bytes).unwrap().unwrap();
        assert_eq!(
            parsed.names[0].name_rva,
            RelativeVirtualAddress::new(u32::MAX - 7)
        );
        assert_eq!(parsed.names[0].utf16le, &[0, 0, 0, 0xd8, 0, 0xdc]);
    }
}
#[test]
fn graph_entry_limit_bounds_zero_length_name_metadata() {
    for plus in [false, true] {
        let name_offset = 16 * 2064;
        let mut bytes = fixture(plus, name_offset + 2);
        for node in 0..16 {
            let rows = (0..256)
                .map(|index| {
                    (
                        0x8000_0000 | name_offset,
                        if node == 0 && index < 15 {
                            0x8000_0000 | ((index + 1) * 2064)
                        } else {
                            0
                        },
                    )
                })
                .collect::<Vec<_>>();
            table(&mut bytes, node * 2064, 256, &rows);
        }
        name(&mut bytes, name_offset, &[]);
        let parsed = parse_pe_resource_directory_names(&bytes).unwrap().unwrap();
        assert_eq!(parsed.names.len(), 4096);
        assert_eq!(
            (
                parsed.names[4095].directory_index,
                parsed.names[4095].entry_index
            ),
            (15, 255)
        );
        assert!(
            parsed
                .names
                .iter()
                .all(|n| n.code_unit_count == 0 && n.utf16le.is_empty())
        );
    }
}
