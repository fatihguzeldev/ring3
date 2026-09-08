use ring3_core::{
    FileOffset, PeExportAddressError, PeExportDirectoryError, PeExportName, PeExportNameError,
    PeExportTarget, PeHeaderError, PeRvaError, RelativeVirtualAddress, parse_pe_export_addresses,
    parse_pe_export_names,
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
fn both_widths_preserve_unsorted_duplicates_coordinates_and_all_target_associations() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 3);
        row(&mut bytes, 0, 0xc000, 2);
        row(&mut bytes, 1, 0xc010, 1);
        let result = parse_pe_export_names(&bytes).unwrap().unwrap();
        assert_eq!(
            result.addresses,
            parse_pe_export_addresses(&bytes).unwrap().unwrap()
        );
        assert_eq!(result.entries.len(), 3);
        for (index, (name_rva, address_index, name)) in (0..).zip([
            (0xc000, 2, "zeta"),
            (0xc010, 1, "Alpha"),
            (0xc000, 0, "zeta"),
        ]) {
            assert_eq!(
                result.entries[usize::try_from(index).unwrap()],
                PeExportName {
                    table_index: index,
                    name_pointer_rva: RelativeVirtualAddress::new(0x5000 + 4 * index),
                    name_pointer_file_offset: FileOffset::new(16896 + 4 * u64::from(index)),
                    ordinal_entry_rva: RelativeVirtualAddress::new(0x9000 + 2 * index),
                    ordinal_entry_file_offset: FileOffset::new(33280 + 2 * u64::from(index)),
                    address_index,
                    name_rva: RelativeVirtualAddress::new(name_rva),
                    name_file_offset: FileOffset::new(u64::from(name_rva) - 0x1000 + 512),
                    name,
                }
            );
        }
        assert!(std::ptr::eq(
            result.entries[0].name.as_ptr(),
            bytes[file_offset(0xc000)..].as_ptr()
        ));
        assert_eq!(result.addresses.entries[2].ordinal, 9);
        assert_eq!(result.addresses.entries[0].target, PeExportTarget::Empty);
    }
}

#[test]
fn absent_directory_and_present_zero_names_or_addresses_remain_distinct() {
    for plus in [false, true] {
        for source in [0, u32::MAX] {
            let mut bytes = fixture(plus, 0);
            put32(&mut bytes, 544, source);
            put32(&mut bytes, 548, source);
            assert!(
                parse_pe_export_names(&bytes)
                    .unwrap()
                    .unwrap()
                    .entries
                    .is_empty()
            );
            put32(&mut bytes, 532, 0);
            put32(&mut bytes, 540, u32::MAX);
            let result = parse_pe_export_names(&bytes).unwrap().unwrap();
            assert!(result.entries.is_empty());
            assert!(result.addresses.entries.is_empty());
            directory(&mut bytes, plus, 0, 0);
            assert_eq!(parse_pe_export_names(&bytes), Ok(None));
            directory(&mut bytes, plus, u32::MAX, u32::MAX);
            put32(&mut bytes, if plus { 260 } else { 244 }, 0);
            assert_eq!(parse_pe_export_names(&bytes), Ok(None));
        }
    }
}

#[test]
fn complete_address_validation_precedes_every_name_policy() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, u32::MAX);
        put32(&mut bytes, 544, 0);
        bytes[768] = 0;
        assert_eq!(
            parse_pe_export_names(&bytes),
            Err(PeExportNameError::Addresses(
                PeExportAddressError::EmptyForwarder {
                    entry_index: 2,
                    start: RelativeVirtualAddress::new(0x1100)
                }
            ))
        );
        put32(&mut bytes, 528, u32::MAX);
        assert_eq!(
            parse_pe_export_names(&bytes),
            Err(PeExportNameError::Addresses(
                PeExportAddressError::OrdinalOverflow {
                    ordinal_base: u32::MAX,
                    entry_index: 1
                }
            ))
        );
        directory(&mut bytes, plus, 0, 0);
        bytes[0] = 0;
        assert_eq!(
            parse_pe_export_names(&bytes),
            Err(PeExportNameError::Addresses(
                PeExportAddressError::Directory(PeExportDirectoryError::Base(PeRvaError::Parse(
                    PeHeaderError::InvalidDosSignature {
                        offset: FileOffset::new(0)
                    }
                )))
            ))
        );
    }
}

#[test]
fn name_limit_is_independent_of_address_count_and_precedes_both_sources() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 4096);
        let result = parse_pe_export_names(&bytes).unwrap().unwrap();
        assert_eq!(result.entries.len(), 4096);
        assert_eq!(result.addresses.entries.len(), 3);
        assert_eq!(result.entries[4095].address_index, 0);
        for count in [4097, u32::MAX] {
            put32(&mut bytes, 536, count);
            put32(&mut bytes, 544, 0);
            put32(&mut bytes, 548, 0);
            assert_eq!(
                parse_pe_export_names(&bytes),
                Err(PeExportNameError::NameLimitExceeded { count, limit: 4096 })
            );
        }
        put32(&mut bytes, 536, 1);
        assert_eq!(
            parse_pe_export_names(&bytes),
            Err(PeExportNameError::NamePointerTableUnavailable { count: 1 })
        );
        put32(&mut bytes, 544, u32::MAX);
        assert_eq!(
            parse_pe_export_names(&bytes),
            Err(PeExportNameError::OrdinalTableUnavailable { count: 1 })
        );
        put32(&mut bytes, 548, u32::MAX);
        let start = RelativeVirtualAddress::new(u32::MAX);
        assert_eq!(
            parse_pe_export_names(&bytes),
            Err(PeExportNameError::NamePointerTableRange {
                start,
                length: 4,
                cause: PeRvaError::RvaRangeOverflow { start, length: 4 },
            })
        );
        put32(&mut bytes, 544, 0x5000);
        assert_eq!(
            parse_pe_export_names(&bytes),
            Err(PeExportNameError::OrdinalTableRange {
                start,
                length: 2,
                cause: PeRvaError::RvaRangeOverflow { start, length: 2 },
            })
        );
    }
}

fn table_error(
    ordinal: bool,
    start: RelativeVirtualAddress,
    length: u32,
    cause: PeRvaError,
) -> PeExportNameError {
    if ordinal {
        PeExportNameError::OrdinalTableRange {
            start,
            length,
            cause,
        }
    } else {
        PeExportNameError::NamePointerTableRange {
            start,
            length,
            cause,
        }
    }
}

#[test]
fn both_complete_tables_resolve_before_any_index_or_string() {
    for plus in [false, true] {
        for ordinal in [false, true] {
            let start = RelativeVirtualAddress::new(if ordinal { 0x9000 } else { 0x5000 });
            let length = if ordinal { 4 } else { 8 };
            for (virtual_size, raw_size, cause) in [
                (
                    length - 1,
                    length - 1,
                    PeRvaError::CrossesRegionBoundary { start, length },
                ),
                (
                    0,
                    length,
                    PeRvaError::ZeroVirtualSizeUnsupported {
                        start,
                        length,
                        section_index: 1,
                    },
                ),
                (
                    length - 1,
                    length,
                    PeRvaError::RawPaddingUnsupported {
                        start,
                        length,
                        section_index: 1,
                    },
                ),
                (
                    length,
                    length - 1,
                    PeRvaError::NotFileBacked {
                        start,
                        length,
                        section_index: 1,
                    },
                ),
            ] {
                let mut bytes = fixture(plus, 2);
                row(&mut bytes, 0, 0, u16::MAX);
                let extent = start.get() - 0x1000;
                section(&mut bytes, plus, 0, [extent, 0x1000, extent, 512]);
                section(
                    &mut bytes,
                    plus,
                    1,
                    [virtual_size, start.get(), raw_size, 64_000],
                );
                assert_eq!(
                    parse_pe_export_names(&bytes),
                    Err(table_error(ordinal, start, length, cause))
                );
            }
        }
    }
}

#[test]
fn both_tables_reject_ambiguity_stitching_and_unmapped_ranges() {
    for plus in [false, true] {
        for ordinal in [false, true] {
            let start = RelativeVirtualAddress::new(if ordinal { 0x9000 } else { 0x5000 });
            let length = if ordinal { 4 } else { 8 };
            let mut bytes = fixture(plus, 2);
            section(
                &mut bytes,
                plus,
                1,
                [1, start.get() + length - 1, 1, 64_000],
            );
            assert_eq!(
                parse_pe_export_names(&bytes),
                Err(table_error(
                    ordinal,
                    start,
                    length,
                    PeRvaError::AmbiguousRange { start, length }
                ))
            );
            let extent = start.get() - 0x1000 + length / 2;
            section(&mut bytes, plus, 0, [extent, 0x1000, extent, 512]);
            section(
                &mut bytes,
                plus,
                1,
                [length / 2, start.get() + length / 2, length / 2, 64_000],
            );
            assert_eq!(
                parse_pe_export_names(&bytes),
                Err(table_error(
                    ordinal,
                    start,
                    length,
                    PeRvaError::CrossesRegionBoundary { start, length }
                ))
            );
            section(
                &mut bytes,
                plus,
                0,
                [start.get() - 0x1000, 0x1000, start.get() - 0x1000, 512],
            );
            section(&mut bytes, plus, 1, [0, 0, 0, 0]);
            assert_eq!(
                parse_pe_export_names(&bytes),
                Err(table_error(
                    ordinal,
                    start,
                    length,
                    PeRvaError::UnmappedRva { start, length }
                ))
            );
        }
    }
}

#[test]
fn address_indexes_are_unbiased_and_checked_before_names() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 1);
        for address_count in [0, 3, 4096] {
            put32(&mut bytes, 532, address_count);
            for address_index in [4096, u16::MAX] {
                row(&mut bytes, 0, 0, address_index);
                assert_eq!(
                    parse_pe_export_names(&bytes),
                    Err(PeExportNameError::AddressIndexOutOfRange {
                        entry_index: 0,
                        address_index,
                        address_count,
                    })
                );
            }
        }
        row(&mut bytes, 0, 0xc000, 4095);
        let result = parse_pe_export_names(&bytes).unwrap().unwrap();
        assert_eq!(result.entries[0].address_index, 4095);
        assert_eq!(result.addresses.entries[4095].ordinal, 4102);
        row(&mut bytes, 0, 0, 0);
        assert_eq!(
            parse_pe_export_names(&bytes),
            Err(PeExportNameError::NameUnavailable { entry_index: 0 })
        );
        put32(&mut bytes, 532, 0);
        assert_eq!(
            parse_pe_export_names(&bytes),
            Err(PeExportNameError::AddressIndexOutOfRange {
                entry_index: 0,
                address_index: 0,
                address_count: 0,
            })
        );
    }
}

#[test]
fn earlier_string_failure_precedes_a_later_invalid_index() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 2);
        bytes[file_offset(0xc000)] = 0;
        row(&mut bytes, 1, 0, u16::MAX);
        assert_eq!(
            parse_pe_export_names(&bytes),
            Err(PeExportNameError::EmptyName {
                entry_index: 0,
                start: RelativeVirtualAddress::new(0xc000),
            })
        );
    }
}

#[test]
fn overlapping_tables_and_header_names_are_not_restricted_by_directory_extent() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 1);
        bytes[400..404].copy_from_slice(b"Hdr\0");
        row(&mut bytes, 0, 400, 0);
        put32(&mut bytes, 548, 0x5002);
        let result = parse_pe_export_names(&bytes).unwrap().unwrap();
        assert_eq!(result.entries[0].name, "Hdr");
        assert_eq!(result.entries[0].name_file_offset, FileOffset::new(400));
        assert_eq!(
            result.entries[0].ordinal_entry_rva,
            RelativeVirtualAddress::new(0x5002)
        );
        assert!(std::ptr::eq(
            result.entries[0].name.as_ptr(),
            bytes[400..].as_ptr()
        ));
    }
}

#[test]
fn unusual_ascii_is_borrowed_and_post_nul_bytes_are_ignored() {
    for plus in [false, true] {
        for name in [" ", ".#", "\t\n\u{1}\u{7f}"] {
            let mut bytes = fixture(plus, 1);
            let offset = file_offset(0xc000);
            bytes[offset..offset + name.len()].copy_from_slice(name.as_bytes());
            bytes[offset + name.len()] = 0;
            bytes[offset + name.len() + 1] = 0xff;
            assert_eq!(
                parse_pe_export_names(&bytes).unwrap().unwrap().entries[0].name,
                name
            );
        }
        let mut bytes = fixture(plus, 1);
        bytes[file_offset(0xc000)..file_offset(0xc000) + 3].copy_from_slice(&[b'A', 0x80, 0xff]);
        assert_eq!(
            parse_pe_export_names(&bytes),
            Err(PeExportNameError::NonAsciiName {
                entry_index: 0,
                start: RelativeVirtualAddress::new(0xc000),
                offset: 1,
                byte: 0x80,
            })
        );
    }
}

#[test]
fn whole_name_prefixes_keep_conservative_ownership_and_overflow_errors() {
    let start = RelativeVirtualAddress::new(0xc000);
    for plus in [false, true] {
        for (virtual_size, raw_size, cause) in [
            (
                0xb001,
                0xb001,
                PeRvaError::CrossesRegionBoundary { start, length: 2 },
            ),
            (
                0xb001,
                0xb002,
                PeRvaError::RawPaddingUnsupported {
                    start,
                    length: 2,
                    section_index: 0,
                },
            ),
            (
                0xb002,
                0xb001,
                PeRvaError::NotFileBacked {
                    start,
                    length: 2,
                    section_index: 0,
                },
            ),
        ] {
            let mut bytes = fixture(plus, 1);
            bytes[file_offset(0xc000)..file_offset(0xc002)].copy_from_slice(b"A\0");
            section(&mut bytes, plus, 0, [virtual_size, 0x1000, raw_size, 512]);
            assert_eq!(
                parse_pe_export_names(&bytes),
                Err(PeExportNameError::NameRange {
                    entry_index: 0,
                    start,
                    offset: 1,
                    cause,
                })
            );
        }
        let mut bytes = fixture(plus, 1);
        section(&mut bytes, plus, 1, [1, 0xc001, 1, 64_000]);
        assert_eq!(
            parse_pe_export_names(&bytes),
            Err(PeExportNameError::NameRange {
                entry_index: 0,
                start,
                offset: 1,
                cause: PeRvaError::AmbiguousRange { start, length: 2 },
            })
        );
        section(&mut bytes, plus, 0, [0xb001, 0x1000, 0xb001, 512]);
        assert_eq!(
            parse_pe_export_names(&bytes),
            Err(PeExportNameError::NameRange {
                entry_index: 0,
                start,
                offset: 1,
                cause: PeRvaError::CrossesRegionBoundary { start, length: 2 },
            })
        );
        section(&mut bytes, plus, 1, [2, u32::MAX - 1, 2, 64_000]);
        row(&mut bytes, 0, u32::MAX - 1, 0);
        bytes[64_000..64_002].copy_from_slice(b"A\0");
        let result = parse_pe_export_names(&bytes).unwrap().unwrap();
        assert_eq!(result.entries[0].name_file_offset, FileOffset::new(64_000));
        bytes[64_001] = b'A';
        let start = RelativeVirtualAddress::new(u32::MAX - 1);
        assert_eq!(
            parse_pe_export_names(&bytes),
            Err(PeExportNameError::NameRange {
                entry_index: 0,
                start,
                offset: 2,
                cause: PeRvaError::RvaRangeOverflow { start, length: 3 },
            })
        );
    }
}

#[test]
fn local_name_limit_includes_nul_and_precedes_the_next_range() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 1);
        let offset = file_offset(0xc000);
        bytes[offset..offset + 1023].fill(b'A');
        bytes[offset + 1023] = 0;
        section(&mut bytes, plus, 0, [0xb400, 0x1000, 0xb400, 512]);
        assert_eq!(
            parse_pe_export_names(&bytes).unwrap().unwrap().entries[0]
                .name
                .len(),
            1023
        );
        bytes[offset + 1023] = b'A';
        assert_eq!(
            parse_pe_export_names(&bytes),
            Err(PeExportNameError::NameLengthLimitExceeded {
                entry_index: 0,
                start: RelativeVirtualAddress::new(0xc000),
                limit: 1024,
            })
        );
    }
}

#[test]
fn forwarder_and_name_budgets_are_separate_and_duplicates_count_toward_each() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 64);
        put32(&mut bytes, 532, 64);
        bytes[768..1791].fill(b'F');
        bytes[1791] = 0;
        let offset = file_offset(0xc000);
        bytes[offset..offset + 1023].fill(b'N');
        bytes[offset + 1023] = 0;
        for index in 0..64 {
            put32(&mut bytes, file_offset(0x3000 + index * 4), 0x1100);
        }
        let result = parse_pe_export_names(&bytes).unwrap().unwrap();
        assert_eq!(result.entries.len(), 64);
        assert_eq!(result.addresses.entries.len(), 64);
        put32(&mut bytes, 536, 65);
        row(&mut bytes, 64, u32::MAX, 0);
        assert_eq!(
            parse_pe_export_names(&bytes),
            Err(PeExportNameError::NameScanBudgetExceeded {
                entry_index: 64,
                start: RelativeVirtualAddress::new(u32::MAX),
                offset: 0,
                limit: 65_536,
            })
        );
        row(&mut bytes, 64, 0, 0);
        assert_eq!(
            parse_pe_export_names(&bytes),
            Err(PeExportNameError::NameUnavailable { entry_index: 64 })
        );
        row(&mut bytes, 64, 0, u16::MAX);
        assert_eq!(
            parse_pe_export_names(&bytes),
            Err(PeExportNameError::AddressIndexOutOfRange {
                entry_index: 64,
                address_index: u16::MAX,
                address_count: 64,
            })
        );
        row(&mut bytes, 63, 0xc800, 0);
        row(&mut bytes, 64, 0xc000, 0);
        let second = file_offset(0xc800);
        bytes[second..second + 511].fill(b'B');
        bytes[second + 511] = 0;
        assert_eq!(
            parse_pe_export_names(&bytes),
            Err(PeExportNameError::NameScanBudgetExceeded {
                entry_index: 64,
                start: RelativeVirtualAddress::new(0xc000),
                offset: 512,
                limit: 65_536,
            })
        );
        bytes[second..second + 1024].fill(b'B');
        assert_eq!(
            parse_pe_export_names(&bytes),
            Err(PeExportNameError::NameLengthLimitExceeded {
                entry_index: 63,
                start: RelativeVirtualAddress::new(0xc800),
                limit: 1024,
            })
        );
    }
}

#[test]
fn both_table_widths_accept_the_last_rva_byte_without_wrapping() {
    for plus in [false, true] {
        for ordinal in [false, true] {
            let mut bytes = fixture(plus, 1);
            let width = if ordinal { 2 } else { 4 };
            let start = RelativeVirtualAddress::new(u32::MAX - width + 1);
            section(&mut bytes, plus, 1, [width, start.get(), width, 64_000]);
            put32(&mut bytes, if ordinal { 548 } else { 544 }, start.get());
            if !ordinal {
                put32(&mut bytes, 64_000, 0xc000);
            }
            let result = parse_pe_export_names(&bytes).unwrap().unwrap();
            let entry = result.entries[0];
            assert_eq!(
                if ordinal {
                    entry.ordinal_entry_rva
                } else {
                    entry.name_pointer_rva
                },
                start
            );
            assert_eq!(
                if ordinal {
                    entry.ordinal_entry_file_offset
                } else {
                    entry.name_pointer_file_offset
                },
                FileOffset::new(64_000)
            );
            put32(&mut bytes, 536, 2);
            assert_eq!(
                parse_pe_export_names(&bytes),
                Err(table_error(
                    ordinal,
                    start,
                    width * 2,
                    PeRvaError::RvaRangeOverflow {
                        start,
                        length: width * 2
                    }
                ))
            );
        }
    }
}

fn read_fixture(variable: &str) -> Vec<u8> {
    let path = std::env::var_os(variable)
        .expect("an explicit generated export name fixture path is required");
    std::fs::read(path).unwrap()
}

fn check_named_fixture(variable: &str) {
    let bytes = read_fixture(variable);
    let result = parse_pe_export_names(&bytes).unwrap().unwrap();
    assert_eq!(
        result.addresses,
        parse_pe_export_addresses(&bytes).unwrap().unwrap()
    );
    assert_eq!(
        result.entries,
        vec![PeExportName {
            table_index: 0,
            name_pointer_rva: RelativeVirtualAddress::new(8251),
            name_pointer_file_offset: FileOffset::new(1595),
            ordinal_entry_rva: RelativeVirtualAddress::new(8255),
            ordinal_entry_file_offset: FileOffset::new(1599),
            address_index: 0,
            name_rva: RelativeVirtualAddress::new(8257),
            name_file_offset: FileOffset::new(1601),
            name: "ring3_probe",
        }]
    );
    assert_eq!(result.addresses.entries[0].ordinal, 1);
    assert_eq!(
        result.addresses.entries[0].target,
        PeExportTarget::Rva(RelativeVirtualAddress::new(4096))
    );
    assert!(std::ptr::eq(
        result.entries[0].name.as_ptr(),
        bytes[1601..].as_ptr()
    ));
}

fn check_ordinal_fixture(variable: &str) {
    let bytes = read_fixture(variable);
    let result = parse_pe_export_names(&bytes).unwrap().unwrap();
    assert_eq!(
        result.addresses,
        parse_pe_export_addresses(&bytes).unwrap().unwrap()
    );
    assert!(result.entries.is_empty());
    assert_eq!(result.addresses.entries.len(), 1);
    assert_eq!(result.addresses.entries[0].ordinal, 32768);
}

fn check_forwarder_fixture(variable: &str) {
    let bytes = read_fixture(variable);
    let result = parse_pe_export_names(&bytes).unwrap().unwrap();
    assert_eq!(
        result.addresses,
        parse_pe_export_addresses(&bytes).unwrap().unwrap()
    );
    assert_eq!(result.entries.len(), 2);
    for (index, (address_index, name_rva, name_file_offset, name)) in
        (0..).zip([(0, 8276, 1620, "by_name"), (2, 8284, 1628, "by_ordinal")])
    {
        assert_eq!(
            result.entries[usize::try_from(index).unwrap()],
            PeExportName {
                table_index: index,
                name_pointer_rva: RelativeVirtualAddress::new(8264 + index * 4),
                name_pointer_file_offset: FileOffset::new(1608 + u64::from(index) * 4),
                ordinal_entry_rva: RelativeVirtualAddress::new(8272 + index * 2),
                ordinal_entry_file_offset: FileOffset::new(1616 + u64::from(index) * 2),
                address_index,
                name_rva: RelativeVirtualAddress::new(name_rva),
                name_file_offset: FileOffset::new(name_file_offset),
                name,
            }
        );
        assert_eq!(
            result.addresses.entries[usize::from(address_index)].ordinal,
            7 + u32::from(address_index)
        );
        assert!(std::ptr::eq(
            result.entries[usize::try_from(index).unwrap()]
                .name
                .as_ptr(),
            bytes[usize::try_from(name_file_offset).unwrap()..].as_ptr()
        ));
    }
}

#[test]
#[ignore = "requires the explicit RING3_EXPORT_PE32_NAMED_DLL path"]
fn named_pe32_dll_matches_recorded_name_metadata() {
    check_named_fixture("RING3_EXPORT_PE32_NAMED_DLL");
}

#[test]
#[ignore = "requires the explicit RING3_EXPORT_PE32PLUS_NAMED_DLL path"]
fn named_pe32plus_dll_matches_recorded_name_metadata() {
    check_named_fixture("RING3_EXPORT_PE32PLUS_NAMED_DLL");
}

#[test]
#[ignore = "requires the explicit RING3_EXPORT_PE32_ORDINAL_DLL path"]
fn ordinal_pe32_dll_matches_recorded_name_metadata() {
    check_ordinal_fixture("RING3_EXPORT_PE32_ORDINAL_DLL");
}

#[test]
#[ignore = "requires the explicit RING3_EXPORT_PE32PLUS_ORDINAL_DLL path"]
fn ordinal_pe32plus_dll_matches_recorded_name_metadata() {
    check_ordinal_fixture("RING3_EXPORT_PE32PLUS_ORDINAL_DLL");
}

#[test]
#[ignore = "requires the explicit RING3_EXPORT_FORWARD_PE32_FIXTURE path"]
fn forwarder_pe32_dll_matches_recorded_name_metadata() {
    check_forwarder_fixture("RING3_EXPORT_FORWARD_PE32_FIXTURE");
}

#[test]
#[ignore = "requires the explicit RING3_EXPORT_FORWARD_PE32PLUS_FIXTURE path"]
fn forwarder_pe32plus_dll_matches_recorded_name_metadata() {
    check_forwarder_fixture("RING3_EXPORT_FORWARD_PE32PLUS_FIXTURE");
}
