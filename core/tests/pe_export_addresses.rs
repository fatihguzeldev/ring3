use ring3_core::{
    FileOffset, PeExportAddressEntry, PeExportAddressError, PeExportDirectoryError, PeExportTarget,
    PeHeaderError, PeRvaError, RelativeVirtualAddress, parse_pe_export_addresses,
    parse_pe_export_directory,
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

fn entry(bytes: &mut [u8], index: u32, raw: u32) {
    put32(bytes, file_offset(0x3000 + index * 4), raw);
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
    section(&mut bytes, plus, 0, [0xe000, 0x1000, 0xe000, 512]);
    put32(&mut bytes, 528, 7);
    put32(&mut bytes, 532, count);
    put32(&mut bytes, 536, u32::MAX);
    put32(&mut bytes, 540, 0x3000);
    put32(&mut bytes, 544, u32::MAX);
    put32(&mut bytes, 548, u32::MAX);
    bytes[768..789].copy_from_slice(b"OtherModule.Function\0");
    bytes
}

#[test]
fn both_widths_preserve_order_holes_coordinates_ordinals_and_borrowed_forwarders() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 6);
        for (index, raw) in (0..).zip([0, 1, 0x1100, u32::MAX, 0x1100, 0x2000]) {
            entry(&mut bytes, index, raw);
        }
        let result = parse_pe_export_addresses(&bytes).unwrap().unwrap();
        assert_eq!(
            result.directory,
            parse_pe_export_directory(&bytes).unwrap().unwrap()
        );
        assert_eq!(result.entries.len(), 6);
        let forwarder = PeExportTarget::Forwarder {
            rva: RelativeVirtualAddress::new(0x1100),
            text: "OtherModule.Function",
        };
        for (index, target) in (0..).zip([
            PeExportTarget::Empty,
            PeExportTarget::Rva(RelativeVirtualAddress::new(1)),
            forwarder,
            PeExportTarget::Rva(RelativeVirtualAddress::new(u32::MAX)),
            forwarder,
            PeExportTarget::Rva(RelativeVirtualAddress::new(0x2000)),
        ]) {
            assert_eq!(
                result.entries[usize::try_from(index).unwrap()],
                PeExportAddressEntry {
                    table_index: index,
                    ordinal: 7 + index,
                    entry_rva: RelativeVirtualAddress::new(0x3000 + 4 * index),
                    entry_file_offset: FileOffset::new(8704 + 4 * u64::from(index)),
                    target,
                }
            );
        }
        let PeExportTarget::Forwarder { text, .. } = result.entries[2].target else {
            panic!("expected a borrowed forwarder")
        };
        assert!(std::ptr::eq(text.as_ptr(), bytes[768..].as_ptr()));
    }
}

#[test]
fn absent_zero_slot_and_present_zero_count_have_distinct_results() {
    for plus in [false, true] {
        for source in [0, u32::MAX] {
            let mut bytes = fixture(plus, 0);
            put32(&mut bytes, 540, source);
            let result = parse_pe_export_addresses(&bytes).unwrap().unwrap();
            assert!(result.entries.is_empty());
            assert_eq!(
                result.directory.export_address_table_rva,
                RelativeVirtualAddress::new(source)
            );
            directory(&mut bytes, plus, 0, 0);
            assert_eq!(parse_pe_export_addresses(&bytes), Ok(None));
            directory(&mut bytes, plus, u32::MAX, u32::MAX);
            put32(&mut bytes, if plus { 260 } else { 244 }, 0);
            assert_eq!(parse_pe_export_addresses(&bytes), Ok(None));
        }
    }
}

#[test]
fn directory_and_base_failures_precede_entry_policies() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, u32::MAX);
        put32(&mut bytes, 540, 0);
        directory(&mut bytes, plus, 0x1000, 39);
        assert_eq!(
            parse_pe_export_addresses(&bytes),
            Err(PeExportAddressError::Directory(
                PeExportDirectoryError::TruncatedExportDirectory {
                    rva: RelativeVirtualAddress::new(0x1000),
                    size: 39,
                    required: 40,
                }
            ))
        );
        for (rva, size) in [(0, 0), (0x1000, 40)] {
            directory(&mut bytes, plus, rva, size);
            bytes[0] = 0;
            assert_eq!(
                parse_pe_export_addresses(&bytes),
                Err(PeExportAddressError::Directory(
                    PeExportDirectoryError::Base(PeRvaError::Parse(
                        PeHeaderError::InvalidDosSignature {
                            offset: FileOffset::new(0)
                        }
                    ))
                ))
            );
        }
    }
}

#[test]
fn entry_limit_includes_holes_and_precedes_source_or_ordinal_checks() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 4096);
        let result = parse_pe_export_addresses(&bytes).unwrap().unwrap();
        assert_eq!(result.entries.len(), 4096);
        assert!(
            result
                .entries
                .iter()
                .all(|entry| entry.target == PeExportTarget::Empty)
        );
        assert_eq!(result.entries[4095].ordinal, 4102);
        for count in [4097, u32::MAX] {
            put32(&mut bytes, 532, count);
            put32(&mut bytes, 540, 0);
            put32(&mut bytes, 528, u32::MAX);
            assert_eq!(
                parse_pe_export_addresses(&bytes),
                Err(PeExportAddressError::EntryLimitExceeded { count, limit: 4096 })
            );
        }
        put32(&mut bytes, 532, 1);
        assert_eq!(
            parse_pe_export_addresses(&bytes),
            Err(PeExportAddressError::AddressTableUnavailable { count: 1 })
        );
    }
}

#[test]
fn the_complete_table_precedes_entry_ordinals_and_forwarder_bytes() {
    let start = RelativeVirtualAddress::new(0x3000);
    for plus in [false, true] {
        for (fields, cause) in [
            (
                [7, 0x3000, 7, 8704],
                PeRvaError::CrossesRegionBoundary { start, length: 8 },
            ),
            (
                [8, 0x4000, 8, 8704],
                PeRvaError::UnmappedRva { start, length: 8 },
            ),
            (
                [0, 0x3000, 8, 8704],
                PeRvaError::ZeroVirtualSizeUnsupported {
                    start,
                    length: 8,
                    section_index: 1,
                },
            ),
            (
                [7, 0x3000, 8, 8704],
                PeRvaError::RawPaddingUnsupported {
                    start,
                    length: 8,
                    section_index: 1,
                },
            ),
            (
                [8, 0x3000, 7, 8704],
                PeRvaError::NotFileBacked {
                    start,
                    length: 8,
                    section_index: 1,
                },
            ),
        ] {
            let mut bytes = fixture(plus, 2);
            put32(&mut bytes, 528, u32::MAX);
            entry(&mut bytes, 0, 0x1100);
            bytes[768] = 0;
            section(&mut bytes, plus, 0, [0x1000, 0x1000, 0x1000, 512]);
            section(&mut bytes, plus, 1, fields);
            assert_eq!(
                parse_pe_export_addresses(&bytes),
                Err(PeExportAddressError::AddressTableRange {
                    start,
                    length: 8,
                    cause
                })
            );
        }
        let mut bytes = fixture(plus, 2);
        section(&mut bytes, plus, 1, [1, 0x3007, 1, 9000]);
        assert_eq!(
            parse_pe_export_addresses(&bytes),
            Err(PeExportAddressError::AddressTableRange {
                start,
                length: 8,
                cause: PeRvaError::AmbiguousRange { start, length: 8 },
            })
        );
        section(&mut bytes, plus, 0, [0x2004, 0x1000, 0x2004, 512]);
        section(&mut bytes, plus, 1, [4, 0x3004, 4, 8708]);
        assert_eq!(
            parse_pe_export_addresses(&bytes),
            Err(PeExportAddressError::AddressTableRange {
                start,
                length: 8,
                cause: PeRvaError::CrossesRegionBoundary { start, length: 8 },
            })
        );
    }
}

#[test]
fn the_last_table_byte_is_representable_and_overflow_is_typed() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 1);
        let start = RelativeVirtualAddress::new(u32::MAX - 3);
        section(&mut bytes, plus, 1, [4, start.get(), 4, 64_000]);
        put32(&mut bytes, 540, start.get());
        let result = parse_pe_export_addresses(&bytes).unwrap().unwrap();
        assert_eq!(result.entries[0].entry_rva, start);
        assert_eq!(result.entries[0].entry_file_offset, FileOffset::new(64_000));
        put32(&mut bytes, 532, 2);
        assert_eq!(
            parse_pe_export_addresses(&bytes),
            Err(PeExportAddressError::AddressTableRange {
                start,
                length: 8,
                cause: PeRvaError::RvaRangeOverflow { start, length: 8 },
            })
        );
    }
}

#[test]
fn checked_ordinals_apply_to_holes_before_their_targets_but_after_prior_strings() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 2);
        put32(&mut bytes, 528, u32::MAX);
        for raw in [0, 0x1100, u32::MAX] {
            entry(&mut bytes, 1, raw);
            assert_eq!(
                parse_pe_export_addresses(&bytes),
                Err(PeExportAddressError::OrdinalOverflow {
                    ordinal_base: u32::MAX,
                    entry_index: 1
                })
            );
        }
        entry(&mut bytes, 0, 0x1100);
        bytes[768] = 0;
        assert_eq!(
            parse_pe_export_addresses(&bytes),
            Err(PeExportAddressError::EmptyForwarder {
                entry_index: 0,
                start: RelativeVirtualAddress::new(0x1100)
            })
        );
        entry(&mut bytes, 0, 0);
        entry(&mut bytes, 1, u32::MAX);
        for base in [0, 65535, u32::MAX - 1] {
            put32(&mut bytes, 528, base);
            let result = parse_pe_export_addresses(&bytes).unwrap().unwrap();
            assert_eq!(result.entries[0].ordinal, base);
            assert_eq!(result.entries[1].ordinal, base + 1);
        }
    }
}

#[test]
fn classification_uses_the_half_open_directory_range_without_target_reads() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 3);
        put32(&mut bytes, 512, u32::from(b'X'));
        for (index, raw) in (0..).zip([0xfff, 0x1000, 0x2000]) {
            entry(&mut bytes, index, raw);
        }
        let result = parse_pe_export_addresses(&bytes).unwrap().unwrap();
        assert_eq!(
            result.entries[0].target,
            PeExportTarget::Rva(RelativeVirtualAddress::new(0xfff))
        );
        assert_eq!(
            result.entries[1].target,
            PeExportTarget::Forwarder {
                rva: RelativeVirtualAddress::new(0x1000),
                text: "X"
            }
        );
        assert_eq!(
            result.entries[2].target,
            PeExportTarget::Rva(RelativeVirtualAddress::new(0x2000))
        );
        section(&mut bytes, plus, 1, [0, u32::MAX, 1, 64_000]);
        entry(&mut bytes, 2, u32::MAX);
        assert_eq!(
            parse_pe_export_addresses(&bytes).unwrap().unwrap().entries[2].target,
            PeExportTarget::Rva(RelativeVirtualAddress::new(u32::MAX))
        );
    }
}

#[test]
fn raw_ascii_is_retained_without_forwarder_grammar_checks() {
    for plus in [false, true] {
        for text in ["OtherModule.#32768", " ", ".#", "\t\n\u{1}\u{7f}"] {
            let mut bytes = fixture(plus, 1);
            entry(&mut bytes, 0, 0x1100);
            bytes[768..768 + text.len()].copy_from_slice(text.as_bytes());
            bytes[768 + text.len()] = 0;
            bytes[769 + text.len()] = 0xff;
            assert_eq!(
                parse_pe_export_addresses(&bytes).unwrap().unwrap().entries[0].target,
                PeExportTarget::Forwarder {
                    rva: RelativeVirtualAddress::new(0x1100),
                    text
                }
            );
        }
        let mut bytes = fixture(plus, 1);
        entry(&mut bytes, 0, 0x1100);
        bytes[768] = 0;
        assert_eq!(
            parse_pe_export_addresses(&bytes),
            Err(PeExportAddressError::EmptyForwarder {
                entry_index: 0,
                start: RelativeVirtualAddress::new(0x1100),
            })
        );
        bytes[768..771].copy_from_slice(&[b'A', 0x80, 0xff]);
        assert_eq!(
            parse_pe_export_addresses(&bytes),
            Err(PeExportAddressError::NonAsciiForwarder {
                entry_index: 0,
                start: RelativeVirtualAddress::new(0x1100),
                offset: 1,
                byte: 0x80,
            })
        );
    }
}

#[test]
fn forwarder_whole_prefixes_preserve_physical_refusals() {
    let start = RelativeVirtualAddress::new(0x1100);
    for plus in [false, true] {
        for (virtual_size, raw_size, cause) in [
            (
                0x101,
                0x101,
                PeRvaError::CrossesRegionBoundary { start, length: 2 },
            ),
            (
                0x101,
                0x102,
                PeRvaError::RawPaddingUnsupported {
                    start,
                    length: 2,
                    section_index: 0,
                },
            ),
            (
                0x102,
                0x101,
                PeRvaError::NotFileBacked {
                    start,
                    length: 2,
                    section_index: 0,
                },
            ),
        ] {
            let mut bytes = fixture(plus, 1);
            entry(&mut bytes, 0, start.get());
            bytes[768..770].copy_from_slice(b"A\0");
            section(&mut bytes, plus, 0, [virtual_size, 0x1000, raw_size, 512]);
            section(&mut bytes, plus, 1, [4, 0x3000, 4, 8704]);
            assert_eq!(
                parse_pe_export_addresses(&bytes),
                Err(PeExportAddressError::ForwarderRange {
                    entry_index: 0,
                    start,
                    offset: 1,
                    cause,
                })
            );
        }
        let mut bytes = fixture(plus, 1);
        entry(&mut bytes, 0, start.get());
        section(&mut bytes, plus, 1, [1, 0x1101, 1, 64_000]);
        assert_eq!(
            parse_pe_export_addresses(&bytes),
            Err(PeExportAddressError::ForwarderRange {
                entry_index: 0,
                start,
                offset: 1,
                cause: PeRvaError::AmbiguousRange { start, length: 2 },
            })
        );
        bytes[134..136].copy_from_slice(&3_u16.to_le_bytes());
        section(&mut bytes, plus, 0, [0x101, 0x1000, 0x101, 512]);
        section(&mut bytes, plus, 1, [1, 0x1101, 1, 769]);
        section(&mut bytes, plus, 2, [4, 0x3000, 4, 8704]);
        bytes[769] = 0;
        assert_eq!(
            parse_pe_export_addresses(&bytes),
            Err(PeExportAddressError::ForwarderRange {
                entry_index: 0,
                start,
                offset: 1,
                cause: PeRvaError::CrossesRegionBoundary { start, length: 2 },
            })
        );
    }
}

#[test]
fn directory_end_includes_nul_and_precedes_the_next_physical_read() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 1);
        entry(&mut bytes, 0, 0x1100);
        bytes[768..770].copy_from_slice(b"A\0");
        directory(&mut bytes, plus, 0x1000, 0x102);
        assert!(parse_pe_export_addresses(&bytes).is_ok());
        directory(&mut bytes, plus, 0x1000, 0x101);
        section(&mut bytes, plus, 0, [0x101, 0x1000, 0x101, 512]);
        section(&mut bytes, plus, 1, [4, 0x3000, 4, 8704]);
        assert_eq!(
            parse_pe_export_addresses(&bytes),
            Err(PeExportAddressError::UnterminatedForwarder {
                entry_index: 0,
                start: RelativeVirtualAddress::new(0x1100),
                offset: 1,
                directory_end: 0x1101,
            })
        );
        let mut edge = fixture(plus, 1);
        let start = RelativeVirtualAddress::new(u32::MAX - 1);
        directory(&mut edge, plus, 0x1000, 0xffff_f000);
        section(&mut edge, plus, 1, [2, start.get(), 2, 64_000]);
        entry(&mut edge, 0, start.get());
        edge[64_000..64_002].copy_from_slice(b"A\0");
        assert!(parse_pe_export_addresses(&edge).is_ok());
        edge[64_001] = b'A';
        assert_eq!(
            parse_pe_export_addresses(&edge),
            Err(PeExportAddressError::UnterminatedForwarder {
                entry_index: 0,
                start,
                offset: 2,
                directory_end: 1_u64 << 32,
            })
        );
    }
}

#[test]
fn local_limit_counts_nul_and_precedes_directory_end_and_physical_bytes() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 1);
        entry(&mut bytes, 0, 0x1100);
        bytes[768..1791].fill(b'A');
        bytes[1791] = 0;
        directory(&mut bytes, plus, 0x1000, 0x500);
        section(&mut bytes, plus, 0, [0x500, 0x1000, 0x500, 512]);
        section(&mut bytes, plus, 1, [4, 0x3000, 4, 8704]);
        let result = parse_pe_export_addresses(&bytes).unwrap().unwrap();
        let PeExportTarget::Forwarder { text, .. } = result.entries[0].target else {
            panic!("expected a forwarder")
        };
        assert_eq!(text.len(), 1023);
        bytes[1791] = b'A';
        assert_eq!(
            parse_pe_export_addresses(&bytes),
            Err(PeExportAddressError::ForwarderLengthLimitExceeded {
                entry_index: 0,
                start: RelativeVirtualAddress::new(0x1100),
                limit: 1024,
            })
        );
    }
}

#[test]
fn global_budget_counts_duplicate_scans_and_accepts_its_last_nul() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 66);
        bytes[768..1791].fill(b'A');
        bytes[1791] = 0;
        for index in 0..64 {
            entry(&mut bytes, index, 0x1100);
        }
        entry(&mut bytes, 64, u32::MAX);
        assert_eq!(
            parse_pe_export_addresses(&bytes)
                .unwrap()
                .unwrap()
                .entries
                .len(),
            66
        );
        entry(&mut bytes, 65, 0x1fff);
        section(&mut bytes, plus, 0, [0x500, 0x1000, 0x500, 512]);
        section(&mut bytes, plus, 1, [264, 0x3000, 264, 8704]);
        assert_eq!(
            parse_pe_export_addresses(&bytes),
            Err(PeExportAddressError::ForwarderScanBudgetExceeded {
                entry_index: 65,
                start: RelativeVirtualAddress::new(0x1fff),
                offset: 0,
                limit: 65_536,
            })
        );
        section(&mut bytes, plus, 0, [0xe000, 0x1000, 0xe000, 512]);
        section(&mut bytes, plus, 1, [0, 0, 0, 0]);
        let second = file_offset(0x1800);
        bytes[second..second + 511].fill(b'B');
        bytes[second + 511] = 0;
        entry(&mut bytes, 63, 0x1800);
        entry(&mut bytes, 64, 0x1e00);
        bytes[file_offset(0x1e00)..file_offset(0x2000)].fill(b'C');
        assert_eq!(
            parse_pe_export_addresses(&bytes),
            Err(PeExportAddressError::ForwarderScanBudgetExceeded {
                entry_index: 64,
                start: RelativeVirtualAddress::new(0x1e00),
                offset: 512,
                limit: 65_536,
            })
        );
        bytes[second..second + 1024].fill(b'B');
        assert_eq!(
            parse_pe_export_addresses(&bytes),
            Err(PeExportAddressError::ForwarderLengthLimitExceeded {
                entry_index: 63,
                start: RelativeVirtualAddress::new(0x1800),
                limit: 1024,
            })
        );
    }
}

fn read_fixture(variable: &str) -> Vec<u8> {
    let path = std::env::var_os(variable)
        .expect("an explicit generated export address fixture path is required");
    std::fs::read(path).unwrap()
}

fn check_direct_fixture(variable: &str, named: bool) {
    let bytes = read_fixture(variable);
    let result = parse_pe_export_addresses(&bytes).unwrap().unwrap();
    assert_eq!(
        result.directory,
        parse_pe_export_directory(&bytes).unwrap().unwrap()
    );
    assert_eq!(
        result.entries,
        vec![PeExportAddressEntry {
            table_index: 0,
            ordinal: if named { 1 } else { 32768 },
            entry_rva: RelativeVirtualAddress::new(if named { 8247 } else { 8249 }),
            entry_file_offset: FileOffset::new(if named { 1591 } else { 1593 }),
            target: PeExportTarget::Rva(RelativeVirtualAddress::new(4096)),
        }]
    );
}

fn check_forwarder_fixture(variable: &str) {
    let bytes = read_fixture(variable);
    let result = parse_pe_export_addresses(&bytes).unwrap().unwrap();
    assert_eq!(
        result.directory,
        parse_pe_export_directory(&bytes).unwrap().unwrap()
    );
    assert_eq!(
        result.directory.directory_rva,
        RelativeVirtualAddress::new(8192)
    );
    assert_eq!(result.directory.directory_size, 147);
    assert_eq!(
        result.entries,
        vec![
            PeExportAddressEntry {
                table_index: 0,
                ordinal: 7,
                entry_rva: RelativeVirtualAddress::new(8252),
                entry_file_offset: FileOffset::new(1596),
                target: PeExportTarget::Forwarder {
                    rva: RelativeVirtualAddress::new(8295),
                    text: "OtherModule.ring3_target"
                },
            },
            PeExportAddressEntry {
                table_index: 1,
                ordinal: 8,
                entry_rva: RelativeVirtualAddress::new(8256),
                entry_file_offset: FileOffset::new(1600),
                target: PeExportTarget::Empty,
            },
            PeExportAddressEntry {
                table_index: 2,
                ordinal: 9,
                entry_rva: RelativeVirtualAddress::new(8260),
                entry_file_offset: FileOffset::new(1604),
                target: PeExportTarget::Forwarder {
                    rva: RelativeVirtualAddress::new(8320),
                    text: "OtherModule.#32768"
                },
            },
        ]
    );
    for (index, offset) in [(0, 1639), (2, 1664)] {
        let PeExportTarget::Forwarder { text, .. } = result.entries[index].target else {
            panic!("expected a borrowed forwarder")
        };
        assert!(std::ptr::eq(text.as_ptr(), bytes[offset..].as_ptr()));
    }
}

#[test]
#[ignore = "requires the explicit RING3_EXPORT_PE32_NAMED_DLL path"]
fn named_pe32_dll_matches_recorded_address_metadata() {
    check_direct_fixture("RING3_EXPORT_PE32_NAMED_DLL", true);
}

#[test]
#[ignore = "requires the explicit RING3_EXPORT_PE32PLUS_NAMED_DLL path"]
fn named_pe32plus_dll_matches_recorded_address_metadata() {
    check_direct_fixture("RING3_EXPORT_PE32PLUS_NAMED_DLL", true);
}

#[test]
#[ignore = "requires the explicit RING3_EXPORT_PE32_ORDINAL_DLL path"]
fn ordinal_pe32_dll_matches_recorded_address_metadata() {
    check_direct_fixture("RING3_EXPORT_PE32_ORDINAL_DLL", false);
}

#[test]
#[ignore = "requires the explicit RING3_EXPORT_PE32PLUS_ORDINAL_DLL path"]
fn ordinal_pe32plus_dll_matches_recorded_address_metadata() {
    check_direct_fixture("RING3_EXPORT_PE32PLUS_ORDINAL_DLL", false);
}

#[test]
#[ignore = "requires the explicit RING3_EXPORT_FORWARD_PE32_FIXTURE path"]
fn forwarder_pe32_dll_matches_recorded_sparse_address_metadata() {
    check_forwarder_fixture("RING3_EXPORT_FORWARD_PE32_FIXTURE");
}

#[test]
#[ignore = "requires the explicit RING3_EXPORT_FORWARD_PE32PLUS_FIXTURE path"]
fn forwarder_pe32plus_dll_matches_recorded_sparse_address_metadata() {
    check_forwarder_fixture("RING3_EXPORT_FORWARD_PE32PLUS_FIXTURE");
}
