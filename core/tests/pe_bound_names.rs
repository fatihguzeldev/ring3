use ring3_core::{
    FileOffset, PeBoundImportError, PeBoundImportName, PeBoundImportNameError as Error,
    PeBoundImportNameLocation as Location, PeRvaError, RelativeVirtualAddress as Rva,
    parse_pe_bound_import_descriptors, parse_pe_bound_import_names,
};

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn fixed(plus: bool) -> usize {
    if plus { 112 } else { 96 }
}

fn directory(bytes: &mut [u8], plus: bool, rva: u32, size: u32) {
    put32(bytes, 152 + fixed(plus) + 88, rva);
    put32(bytes, 152 + fixed(plus) + 92, size);
}

fn section(bytes: &mut [u8], plus: bool, index: usize, fields: [u32; 4]) {
    let table = 152 + fixed(plus) + 128 + index * 40;
    for (offset, value) in [8, 12, 16, 20].into_iter().zip(fields) {
        put32(bytes, table + offset, value);
    }
}

fn fixture(plus: bool, size: u32) -> Vec<u8> {
    let mut bytes = vec![0; 12800];
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 60, 128);
    bytes[128..132].copy_from_slice(b"PE\0\0");
    bytes[134..136].copy_from_slice(&2_u16.to_le_bytes());
    bytes[148..150].copy_from_slice(&u16::try_from(fixed(plus) + 128).unwrap().to_le_bytes());
    bytes[152..154].copy_from_slice(&if plus { 0x20b_u16 } else { 0x10b }.to_le_bytes());
    put32(&mut bytes, 212, 512);
    put32(&mut bytes, 152 + fixed(plus) - 4, 16);
    directory(&mut bytes, plus, 4096, size);
    section(&mut bytes, plus, 0, [12288, 4096, 12288, 512]);
    section(&mut bytes, plus, 1, [0, 0x9000, 0, 12800]);
    bytes
}

fn record(bytes: &mut [u8], offset: usize, stamp: u32, name: u16, last: u16) {
    put32(bytes, offset, stamp);
    bytes[offset + 4..offset + 6].copy_from_slice(&name.to_le_bytes());
    bytes[offset + 6..offset + 8].copy_from_slice(&last.to_le_bytes());
}

fn descriptor(index: u16) -> Location {
    Location::Descriptor {
        descriptor_index: index,
    }
}

fn forwarder(index: u16) -> Location {
    Location::Forwarder {
        descriptor_index: 0,
        forwarder_index: index,
    }
}

fn name(bytes: &mut [u8], offset: u16, text: &[u8]) {
    let start = 512 + usize::from(offset);
    bytes[start..start + text.len()].copy_from_slice(text);
    bytes[start + text.len()] = 0;
}

fn row(location: Location, offset: u16, text: &str) -> PeBoundImportName<'_> {
    PeBoundImportName {
        location,
        name_rva: Rva::new(4096 + u32::from(offset)),
        name_file_offset: FileOffset::new(512 + u64::from(offset)),
        dll_name: text,
    }
}

#[test]
fn ordered_names_borrow_exact_bytes_and_keep_the_complete_raw_graph() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 48);
        record(&mut bytes, 512, u32::MAX, 64, 2);
        record(&mut bytes, 520, 7, 80, u16::MAX);
        record(&mut bytes, 528, 0, 64, 0);
        record(&mut bytes, 536, 9, 96, 1);
        record(&mut bytes, 544, 11, 97, 17);
        name(&mut bytes, 64, b"MiXeD.DLL");
        name(&mut bytes, 80, b"a/b\\c\x01\x7f");
        name(&mut bytes, 96, b"abc");
        let before = bytes.clone();
        let output = parse_pe_bound_import_names(&bytes).unwrap().unwrap();
        assert_eq!(
            output.table,
            parse_pe_bound_import_descriptors(&bytes).unwrap().unwrap()
        );
        assert_eq!(
            output.names,
            vec![
                row(descriptor(0), 64, "MiXeD.DLL"),
                row(forwarder(0), 80, "a/b\\c\x01\x7f"),
                row(forwarder(1), 64, "MiXeD.DLL"),
                row(descriptor(1), 96, "abc"),
                row(
                    Location::Forwarder {
                        descriptor_index: 1,
                        forwarder_index: 0
                    },
                    97,
                    "bc"
                ),
            ]
        );
        for entry in &output.names {
            let offset = usize::try_from(entry.name_file_offset.get()).unwrap();
            assert_eq!(entry.dll_name.as_ptr(), bytes[offset..].as_ptr());
        }
        assert_eq!(Some(output), parse_pe_bound_import_names(&bytes).unwrap());
        assert_eq!(bytes, before);
    }
}

#[test]
fn base_absence_and_empty_are_distinct_and_late_raw_errors_win() {
    assert!(matches!(
        parse_pe_bound_import_names(&[]),
        Err(Error::Table(PeBoundImportError::Base(_)))
    ));
    for plus in [false, true] {
        let mut bytes = fixture(plus, 8);
        let empty = parse_pe_bound_import_names(&bytes).unwrap().unwrap();
        assert!(empty.names.is_empty());
        assert_eq!(
            Some(empty.table),
            parse_pe_bound_import_descriptors(&bytes).unwrap()
        );
        record(&mut bytes, 512, 1, u16::MAX, 0);
        assert_eq!(
            parse_pe_bound_import_names(&bytes),
            Err(Error::Table(PeBoundImportError::MissingTerminator {
                descriptor_index: 1
            }))
        );
        directory(&mut bytes, plus, 0, 0);
        assert_eq!(parse_pe_bound_import_names(&bytes), Ok(None));
        directory(&mut bytes, plus, u32::MAX, u32::MAX);
        put32(&mut bytes, 152 + fixed(plus) - 4, 11);
        assert_eq!(parse_pe_bound_import_names(&bytes), Ok(None));
    }
}

#[test]
fn name_starts_may_be_raw_records_headers_or_a_separate_backed_region() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 24);
        record(&mut bytes, 512, 65, 0, 1);
        record(&mut bytes, 520, 66, 8, 0);
        let output = parse_pe_bound_import_names(&bytes).unwrap().unwrap();
        assert_eq!(
            output.names,
            vec![row(descriptor(0), 0, "A"), row(forwarder(0), 8, "B")]
        );
        let mut bytes = fixture(plus, 16);
        directory(&mut bytes, plus, 480, 16);
        record(&mut bytes, 480, 1, 24, 0);
        bytes[504..506].copy_from_slice(b"H\0");
        let output = parse_pe_bound_import_names(&bytes).unwrap().unwrap();
        assert_eq!(output.names[0].dll_name, "H");
        assert_eq!(output.names[0].name_file_offset, FileOffset::new(504));
        let mut bytes = fixture(plus, 16);
        section(&mut bytes, plus, 0, [16, 4096, 16, 512]);
        section(&mut bytes, plus, 1, [16, 8192, 16, 1024]);
        record(&mut bytes, 512, 1, 4096, 0);
        bytes[1024..1026].copy_from_slice(b"S\0");
        let output = parse_pe_bound_import_names(&bytes).unwrap().unwrap();
        assert_eq!(output.names[0].dll_name, "S");
        assert_eq!(output.names[0].name_rva, Rva::new(8192));
        assert_eq!(output.names[0].name_file_offset, FileOffset::new(1024));
    }
}

#[test]
fn empty_and_non_ascii_errors_retain_descriptor_or_forwarder_location() {
    for plus in [false, true] {
        for location in [descriptor(0), forwarder(0)] {
            let mut bytes = fixture(plus, 24);
            record(&mut bytes, 512, 1, 64, 1);
            record(&mut bytes, 520, 1, 80, 0);
            name(&mut bytes, 64, b"good");
            name(&mut bytes, 80, b"good");
            let offset = if location == descriptor(0) { 64 } else { 80 };
            let rva = Rva::new(4096 + u32::from(offset));
            name(&mut bytes, offset, b"");
            assert_eq!(
                parse_pe_bound_import_names(&bytes),
                Err(Error::EmptyDllName {
                    location,
                    name_rva: rva
                })
            );
            name(&mut bytes, offset, b"ab\xff");
            assert_eq!(
                parse_pe_bound_import_names(&bytes),
                Err(Error::NonAsciiDllName {
                    location,
                    name_rva: rva,
                    offset: 2,
                    byte: 255
                })
            );
            name(&mut bytes, offset, b"a\0\xff");
            assert!(parse_pe_bound_import_names(&bytes).is_ok());
        }
    }
}

#[test]
fn each_name_requires_one_conservative_prefix_and_backed_bytes() {
    for plus in [false, true] {
        for (second, cause) in [
            (
                [8, 4161, 8, 577],
                PeRvaError::CrossesRegionBoundary {
                    start: Rva::new(4160),
                    length: 2,
                },
            ),
            (
                [8, 4160, 8, 576],
                PeRvaError::AmbiguousRange {
                    start: Rva::new(4160),
                    length: 1,
                },
            ),
        ] {
            let mut bytes = fixture(plus, 16);
            section(&mut bytes, plus, 0, [65, 4096, 65, 512]);
            section(&mut bytes, plus, 1, second);
            record(&mut bytes, 512, 1, 64, 0);
            name(&mut bytes, 64, b"A");
            let offset = u32::from(second[1] != 4160);
            assert_eq!(
                parse_pe_bound_import_names(&bytes),
                Err(Error::NameRange {
                    location: descriptor(0),
                    name_rva: Rva::new(4160),
                    offset,
                    cause
                })
            );
        }
        let mut bytes = fixture(plus, 16);
        record(&mut bytes, 512, 1, 64, 0);
        section(&mut bytes, plus, 0, [128, 4096, 64, 512]);
        assert_eq!(
            parse_pe_bound_import_names(&bytes),
            Err(Error::NameRange {
                location: descriptor(0),
                name_rva: Rva::new(4160),
                offset: 0,
                cause: PeRvaError::NotFileBacked {
                    start: Rva::new(4160),
                    length: 1,
                    section_index: 0
                }
            })
        );
    }
}

#[test]
fn checked_start_overflow_precedes_reads_and_maximum_rva_can_hold_a_byte() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 16);
        directory(&mut bytes, plus, 0xffff_fff0, 16);
        section(&mut bytes, plus, 0, [16, 0xffff_fff0, 16, 512]);
        record(&mut bytes, 512, 1, 16, 0);
        assert_eq!(
            parse_pe_bound_import_names(&bytes),
            Err(Error::NameRvaOverflow {
                location: descriptor(0),
                directory_rva: Rva::new(0xffff_fff0),
                module_name_offset: 16
            })
        );
        record(&mut bytes, 512, 1, 15, 0);
        assert_eq!(
            parse_pe_bound_import_names(&bytes),
            Err(Error::EmptyDllName {
                location: descriptor(0),
                name_rva: Rva::new(u32::MAX)
            })
        );
        let mut bytes = fixture(plus, 16);
        directory(&mut bytes, plus, 0xffff_ffe0, 16);
        section(&mut bytes, plus, 0, [32, 0xffff_ffe0, 32, 512]);
        record(&mut bytes, 512, 1, 31, 0);
        bytes[543] = b'A';
        assert_eq!(
            parse_pe_bound_import_names(&bytes),
            Err(Error::NameRange {
                location: descriptor(0),
                name_rva: Rva::new(u32::MAX),
                offset: 1,
                cause: PeRvaError::RvaRangeOverflow {
                    start: Rva::new(u32::MAX),
                    length: 2
                }
            })
        );
    }
}

#[test]
fn local_scan_budget_counts_nul_and_refuses_before_the_next_byte() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 16);
        record(&mut bytes, 512, 1, 64, 0);
        name(&mut bytes, 64, &vec![b'A'; 1023]);
        assert_eq!(
            parse_pe_bound_import_names(&bytes).unwrap().unwrap().names[0]
                .dll_name
                .len(),
            1023
        );
        bytes[576 + 1023] = b'A';
        for next in [0, 255] {
            bytes[576 + 1024] = next;
            assert_eq!(
                parse_pe_bound_import_names(&bytes),
                Err(Error::NameLengthLimitExceeded {
                    location: descriptor(0),
                    name_rva: Rva::new(4160),
                    limit: 1024
                })
            );
        }
    }
}

fn repeated(plus: bool, count: u16) -> Vec<u8> {
    let mut bytes = fixture(plus, (u32::from(count) + 1) * 8);
    for i in 0..count {
        record(&mut bytes, 512 + usize::from(i) * 8, 1, 2048, 0);
    }
    name(&mut bytes, 2048, &vec![b'A'; 1023]);
    bytes
}

#[test]
fn aggregate_budget_counts_duplicates_and_can_refuse_before_or_inside_a_name() {
    for plus in [false, true] {
        let bytes = repeated(plus, 64);
        assert_eq!(
            parse_pe_bound_import_names(&bytes)
                .unwrap()
                .unwrap()
                .names
                .len(),
            64
        );
        let mut bytes = repeated(plus, 65);
        assert_eq!(
            parse_pe_bound_import_names(&bytes),
            Err(Error::NameScanBudgetExceeded {
                location: descriptor(64),
                name_rva: Rva::new(6144),
                offset: 0,
                limit: 65536
            })
        );
        record(&mut bytes, 512, 1, 4096, 0);
        name(&mut bytes, 4096, &vec![b'B'; 1022]);
        assert_eq!(
            parse_pe_bound_import_names(&bytes),
            Err(Error::NameScanBudgetExceeded {
                location: descriptor(64),
                name_rva: Rva::new(6144),
                offset: 1,
                limit: 65536
            })
        );
        directory(&mut bytes, plus, 4096, 66 * 8);
        record(&mut bytes, 512, 1, 2048, 64);
        for i in 0..64 {
            record(&mut bytes, 520 + i * 8, 1, 2048, 0);
        }
        assert_eq!(
            parse_pe_bound_import_names(&bytes),
            Err(Error::NameScanBudgetExceeded {
                location: forwarder(63),
                name_rva: Rva::new(6144),
                offset: 0,
                limit: 65536
            })
        );
    }
}

#[test]
fn local_limit_and_checked_start_precede_an_exhausted_aggregate_budget() {
    for plus in [false, true] {
        let mut bytes = repeated(plus, 64);
        record(&mut bytes, 512 + 63 * 8, 1, 4096, 0);
        name(&mut bytes, 4096, &vec![b'B'; 1024]);
        assert_eq!(
            parse_pe_bound_import_names(&bytes),
            Err(Error::NameLengthLimitExceeded {
                location: descriptor(63),
                name_rva: Rva::new(8192),
                limit: 1024
            })
        );
        let mut bytes = repeated(plus, 65);
        directory(&mut bytes, plus, 0xffff_8000, 66 * 8);
        section(&mut bytes, plus, 0, [12288, 0xffff_8000, 12288, 512]);
        record(&mut bytes, 512 + 64 * 8, 1, 32768, 0);
        assert_eq!(
            parse_pe_bound_import_names(&bytes),
            Err(Error::NameRvaOverflow {
                location: descriptor(64),
                directory_rva: Rva::new(0xffff_8000),
                module_name_offset: 32768
            })
        );
    }
}

#[test]
fn maximum_raw_graph_keeps_all_1152_name_occurrences() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 9224);
        record(&mut bytes, 512, 1, 10000, 1024);
        for i in 0..1024 {
            record(&mut bytes, 520 + i * 8, 1, 10000, 0);
        }
        for i in 1..128 {
            record(&mut bytes, 512 + (1024 + i) * 8, 1, 10000, 0);
        }
        name(&mut bytes, 10000, b"x");
        let output = parse_pe_bound_import_names(&bytes).unwrap().unwrap();
        assert_eq!(output.names.len(), 1152);
        assert!(output.names.iter().all(|entry| entry.dll_name == "x"));
        assert_eq!(output.names[0].location, descriptor(0));
        assert_eq!(output.names[1024].location, forwarder(1023));
        assert_eq!(output.names[1151].location, descriptor(127));
        assert_eq!(
            Some(output.table),
            parse_pe_bound_import_descriptors(&bytes).unwrap()
        );
    }
}
