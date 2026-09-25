use ring3_core::{
    FileOffset, PeExportDirectory, PeExportDirectoryError, PeHeaderError, PeRvaError,
    RelativeVirtualAddress, parse_pe_export_directory,
};

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
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

fn fixture(plus: bool) -> Vec<u8> {
    let mut bytes = vec![0; 1024];
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 60, 128);
    bytes[128..132].copy_from_slice(b"PE\0\0");
    bytes[134..136].copy_from_slice(&2_u16.to_le_bytes());
    let fixed = if plus { 112_u16 } else { 96 };
    bytes[148..150].copy_from_slice(&(fixed + 8).to_le_bytes());
    bytes[152..154].copy_from_slice(&if plus { 0x20b_u16 } else { 0x10b }.to_le_bytes());
    put32(&mut bytes, 212, 512);
    put32(&mut bytes, 152 + usize::from(fixed) - 4, 1);
    directory(&mut bytes, plus, 0x1000, 40);
    section(&mut bytes, plus, 0, [128, 0x1000, 128, 512]);
    section(&mut bytes, plus, 1, [128, 0x2000, 128, 640]);
    bytes
}

fn zero_record() -> PeExportDirectory {
    PeExportDirectory {
        directory_rva: RelativeVirtualAddress::new(0x1000),
        directory_file_offset: FileOffset::new(512),
        directory_size: 40,
        flags: 0,
        time_date_stamp: 0,
        major_version: 0,
        minor_version: 0,
        name_rva: RelativeVirtualAddress::new(0),
        ordinal_base: 0,
        address_table_entries: 0,
        number_of_name_pointers: 0,
        export_address_table_rva: RelativeVirtualAddress::new(0),
        name_pointer_rva: RelativeVirtualAddress::new(0),
        ordinal_table_rva: RelativeVirtualAddress::new(0),
    }
}

#[test]
fn both_widths_decode_all_raw_fields_without_following_targets_or_requiring_a_dll() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        for (target, value) in bytes[512..552].iter_mut().zip(1..=40) {
            *target = value;
        }
        assert_eq!(
            parse_pe_export_directory(&bytes),
            Ok(Some(PeExportDirectory {
                flags: 0x0403_0201,
                time_date_stamp: 0x0807_0605,
                major_version: 0x0a09,
                minor_version: 0x0c0b,
                name_rva: RelativeVirtualAddress::new(0x100f_0e0d),
                ordinal_base: 0x1413_1211,
                address_table_entries: 0x1817_1615,
                number_of_name_pointers: 0x1c1b_1a19,
                export_address_table_rva: RelativeVirtualAddress::new(0x201f_1e1d),
                name_pointer_rva: RelativeVirtualAddress::new(0x2423_2221),
                ordinal_table_rva: RelativeVirtualAddress::new(0x2827_2625),
                ..zero_record()
            }))
        );
    }
}

#[test]
fn absent_and_zero_slots_differ_from_a_present_zero_record() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        assert_eq!(parse_pe_export_directory(&bytes), Ok(Some(zero_record())));
        directory(&mut bytes, plus, 0, 0);
        assert_eq!(parse_pe_export_directory(&bytes), Ok(None));
        directory(&mut bytes, plus, u32::MAX, u32::MAX);
        put32(&mut bytes, if plus { 260 } else { 244 }, 0);
        assert_eq!(parse_pe_export_directory(&bytes), Ok(None));
    }
}

#[test]
fn maximum_fields_and_contradictory_counts_remain_lossless() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        bytes[512..552].fill(0xff);
        let expected = PeExportDirectory {
            flags: u32::MAX,
            time_date_stamp: u32::MAX,
            major_version: u16::MAX,
            minor_version: u16::MAX,
            name_rva: RelativeVirtualAddress::new(u32::MAX),
            ordinal_base: u32::MAX,
            address_table_entries: u32::MAX,
            number_of_name_pointers: u32::MAX,
            export_address_table_rva: RelativeVirtualAddress::new(u32::MAX),
            name_pointer_rva: RelativeVirtualAddress::new(u32::MAX),
            ordinal_table_rva: RelativeVirtualAddress::new(u32::MAX),
            ..zero_record()
        };
        assert_eq!(parse_pe_export_directory(&bytes), Ok(Some(expected)));
        put32(&mut bytes, 532, 0);
        assert_eq!(
            parse_pe_export_directory(&bytes),
            Ok(Some(PeExportDirectory {
                address_table_entries: 0,
                ..expected
            }))
        );
        for target in [0, 1, 0x1000, 0x1080, u32::MAX] {
            for offset in [12, 28, 32, 36] {
                put32(&mut bytes, 512 + offset, target);
            }
            assert_eq!(
                parse_pe_export_directory(&bytes),
                Ok(Some(PeExportDirectory {
                    address_table_entries: 0,
                    name_rva: RelativeVirtualAddress::new(target),
                    export_address_table_rva: RelativeVirtualAddress::new(target),
                    name_pointer_rva: RelativeVirtualAddress::new(target),
                    ordinal_table_rva: RelativeVirtualAddress::new(target),
                    ..expected
                }))
            );
        }
    }
}

#[test]
fn base_validation_precedes_absence_and_every_directory_error() {
    for plus in [false, true] {
        for (count, rva, size) in [
            (0, 0, 0),
            (1, 0, 0),
            (1, 0, 1),
            (1, u32::MAX, 2),
            (1, 0x1000, 39),
            (1, 0x3000, 40),
        ] {
            let mut bytes = fixture(plus);
            put32(&mut bytes, if plus { 260 } else { 244 }, count);
            directory(&mut bytes, plus, rva, size);
            let minimum = if plus { 352 } else { 336 };
            put32(&mut bytes, 212, minimum - 1);
            assert_eq!(
                parse_pe_export_directory(&bytes),
                Err(PeExportDirectoryError::Base(
                    PeRvaError::InvalidHeaderExtent {
                        size_of_headers: minimum - 1,
                        minimum: u64::from(minimum),
                        file_size: 1024,
                    }
                ))
            );
            bytes[0] = 0;
            assert_eq!(
                parse_pe_export_directory(&bytes),
                Err(PeExportDirectoryError::Base(PeRvaError::Parse(
                    PeHeaderError::InvalidDosSignature {
                        offset: FileOffset::new(0),
                    }
                )))
            );
        }
    }
}

#[test]
fn unrelated_section_validation_still_precedes_an_absent_directory() {
    let mut bytes = fixture(false);
    directory(&mut bytes, false, 0, 0);
    section(&mut bytes, false, 1, [128, 0x2000, 128, 1000]);
    assert_eq!(
        parse_pe_export_directory(&bytes),
        Err(PeExportDirectoryError::Base(PeRvaError::Parse(
            PeHeaderError::SectionRawDataOutOfBounds {
                section_index: 1,
                section_offset: FileOffset::new(296),
                offset: FileOffset::new(1000),
                needed: 128,
                available: 24,
            }
        )))
    );
}

#[test]
fn inconsistent_pairs_precede_minimum_size_and_physical_resolution() {
    for plus in [false, true] {
        for (rva, size) in [(0, 1), (0, u32::MAX), (u32::MAX, 0), (0x1000, 0)] {
            let mut bytes = fixture(plus);
            directory(&mut bytes, plus, rva, size);
            assert_eq!(
                parse_pe_export_directory(&bytes),
                Err(PeExportDirectoryError::InconsistentExportDirectory {
                    rva: RelativeVirtualAddress::new(rva),
                    size,
                })
            );
        }
    }
}

#[test]
fn declared_overflow_precedes_minimum_and_physical_errors() {
    for plus in [false, true] {
        for size in [2, 39, 40, u32::MAX] {
            let mut bytes = fixture(plus);
            directory(&mut bytes, plus, u32::MAX, size);
            assert_eq!(
                parse_pe_export_directory(&bytes),
                Err(PeExportDirectoryError::ExportDirectoryRangeOverflow {
                    rva: RelativeVirtualAddress::new(u32::MAX),
                    size,
                })
            );
        }
    }
}

#[test]
fn every_nonzero_short_declared_size_precedes_an_unmapped_prefix() {
    for plus in [false, true] {
        for size in 1..40 {
            let mut bytes = fixture(plus);
            directory(&mut bytes, plus, 0x3000, size);
            assert_eq!(
                parse_pe_export_directory(&bytes),
                Err(PeExportDirectoryError::TruncatedExportDirectory {
                    rva: RelativeVirtualAddress::new(0x3000),
                    size,
                    required: 40,
                })
            );
        }
    }
}

#[test]
fn last_rva_byte_and_maximum_declared_size_do_not_wrap() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        let rva = u32::MAX - 39;
        section(&mut bytes, plus, 0, [40, rva, 40, 512]);
        directory(&mut bytes, plus, rva, 40);
        assert_eq!(
            parse_pe_export_directory(&bytes),
            Ok(Some(PeExportDirectory {
                directory_rva: RelativeVirtualAddress::new(rva),
                ..zero_record()
            }))
        );
        directory(&mut bytes, plus, rva, 41);
        assert_eq!(
            parse_pe_export_directory(&bytes),
            Err(PeExportDirectoryError::ExportDirectoryRangeOverflow {
                rva: RelativeVirtualAddress::new(rva),
                size: 41,
            })
        );
        directory(&mut bytes, plus, 1, u32::MAX);
        let result = parse_pe_export_directory(&bytes).unwrap().unwrap();
        assert_eq!(result.directory_rva, RelativeVirtualAddress::new(1));
        assert_eq!(result.directory_file_offset, FileOffset::new(1));
        assert_eq!(result.directory_size, u32::MAX);
    }
}

#[test]
fn only_the_40_byte_prefix_must_be_backed_and_unambiguous() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        section(&mut bytes, plus, 0, [40, 0x1000, 40, 512]);
        section(&mut bytes, plus, 1, [128, 0x1028, 128, 640]);
        bytes[552..640].fill(0xff);
        for size in [40, 41, 0xffff_f000] {
            directory(&mut bytes, plus, 0x1000, size);
            assert_eq!(
                parse_pe_export_directory(&bytes),
                Ok(Some(PeExportDirectory {
                    directory_size: size,
                    ..zero_record()
                }))
            );
        }
        section(&mut bytes, plus, 0, [128, 0x1000, 128, 512]);
        assert_eq!(
            parse_pe_export_directory(&bytes),
            Ok(Some(PeExportDirectory {
                directory_size: 0xffff_f000,
                ..zero_record()
            }))
        );
    }
}

#[test]
fn an_exact_header_prefix_is_allowed_but_its_boundary_is_not_crossed() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        directory(&mut bytes, plus, 472, 40);
        assert_eq!(
            parse_pe_export_directory(&bytes),
            Ok(Some(PeExportDirectory {
                directory_rva: RelativeVirtualAddress::new(472),
                directory_file_offset: FileOffset::new(472),
                ..zero_record()
            }))
        );
        directory(&mut bytes, plus, 473, 40);
        let start = RelativeVirtualAddress::new(473);
        assert_eq!(
            parse_pe_export_directory(&bytes),
            Err(PeExportDirectoryError::DirectoryRange {
                start,
                length: 40,
                cause: PeRvaError::CrossesRegionBoundary { start, length: 40 },
            })
        );
    }
}

#[test]
fn conservative_refusals_preserve_the_exact_prefix_request_and_cause() {
    let start = RelativeVirtualAddress::new(0x1000);
    for plus in [false, true] {
        for (fields, cause) in [
            (
                [39, 0x1000, 39, 512],
                PeRvaError::CrossesRegionBoundary { start, length: 40 },
            ),
            (
                [40, 0x2000, 40, 512],
                PeRvaError::UnmappedRva { start, length: 40 },
            ),
            (
                [0, 0x1000, 40, 512],
                PeRvaError::ZeroVirtualSizeUnsupported {
                    start,
                    length: 40,
                    section_index: 0,
                },
            ),
            (
                [39, 0x1000, 40, 512],
                PeRvaError::RawPaddingUnsupported {
                    start,
                    length: 40,
                    section_index: 0,
                },
            ),
            (
                [40, 0x1000, 39, 512],
                PeRvaError::NotFileBacked {
                    start,
                    length: 40,
                    section_index: 0,
                },
            ),
        ] {
            let mut bytes = fixture(plus);
            section(&mut bytes, plus, 0, fields);
            assert_eq!(
                parse_pe_export_directory(&bytes),
                Err(PeExportDirectoryError::DirectoryRange {
                    start,
                    length: 40,
                    cause
                })
            );
        }
    }
}

#[test]
fn adjacent_regions_are_not_stitched_and_last_byte_overlap_is_ambiguous() {
    let start = RelativeVirtualAddress::new(0x1000);
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        section(&mut bytes, plus, 0, [20, 0x1000, 20, 512]);
        section(&mut bytes, plus, 1, [20, 0x1014, 20, 532]);
        assert_eq!(
            parse_pe_export_directory(&bytes),
            Err(PeExportDirectoryError::DirectoryRange {
                start,
                length: 40,
                cause: PeRvaError::CrossesRegionBoundary { start, length: 40 },
            })
        );
        section(&mut bytes, plus, 0, [40, 0x1000, 40, 512]);
        section(&mut bytes, plus, 1, [1, 0x1027, 1, 640]);
        assert_eq!(
            parse_pe_export_directory(&bytes),
            Err(PeExportDirectoryError::DirectoryRange {
                start,
                length: 40,
                cause: PeRvaError::AmbiguousRange { start, length: 40 },
            })
        );
    }
}

#[test]
fn truncated_physical_section_fails_base_validation_before_decoding() {
    let mut bytes = fixture(false);
    section(&mut bytes, false, 0, [40, 0x1000, 40, 512]);
    section(&mut bytes, false, 1, [0, 0, 0, 0]);
    bytes.truncate(552);
    assert_eq!(parse_pe_export_directory(&bytes), Ok(Some(zero_record())));
    bytes.truncate(551);
    assert_eq!(
        parse_pe_export_directory(&bytes),
        Err(PeExportDirectoryError::Base(PeRvaError::Parse(
            PeHeaderError::SectionRawDataOutOfBounds {
                section_index: 0,
                section_offset: FileOffset::new(256),
                offset: FileOffset::new(512),
                needed: 40,
                available: 39,
            }
        )))
    );
}

fn check_real_fixture(variable: &str, named: bool) {
    let path =
        std::env::var_os(variable).expect("an explicit generated export fixture path is required");
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(
        parse_pe_export_directory(&bytes),
        Ok(Some(PeExportDirectory {
            directory_rva: RelativeVirtualAddress::new(8192),
            directory_file_offset: FileOffset::new(1536),
            directory_size: if named { 77 } else { 61 },
            name_rva: RelativeVirtualAddress::new(8232),
            ordinal_base: if named { 1 } else { 32768 },
            address_table_entries: 1,
            number_of_name_pointers: u32::from(named),
            export_address_table_rva: RelativeVirtualAddress::new(if named { 8247 } else { 8249 }),
            name_pointer_rva: RelativeVirtualAddress::new(if named { 8251 } else { 8253 }),
            ordinal_table_rva: RelativeVirtualAddress::new(if named { 8255 } else { 8253 }),
            ..zero_record()
        }))
    );
}

#[test]
#[ignore = "requires the explicit RING3_EXPORT_PE32_NAMED_DLL path"]
fn named_pe32_dll_matches_recorded_export_metadata() {
    check_real_fixture("RING3_EXPORT_PE32_NAMED_DLL", true);
}

#[test]
#[ignore = "requires the explicit RING3_EXPORT_PE32PLUS_NAMED_DLL path"]
fn named_pe32plus_dll_matches_recorded_export_metadata() {
    check_real_fixture("RING3_EXPORT_PE32PLUS_NAMED_DLL", true);
}

#[test]
#[ignore = "requires the explicit RING3_EXPORT_PE32_ORDINAL_DLL path"]
fn ordinal_pe32_dll_matches_recorded_export_metadata() {
    check_real_fixture("RING3_EXPORT_PE32_ORDINAL_DLL", false);
}

#[test]
#[ignore = "requires the explicit RING3_EXPORT_PE32PLUS_ORDINAL_DLL path"]
fn ordinal_pe32plus_dll_matches_recorded_export_metadata() {
    check_real_fixture("RING3_EXPORT_PE32PLUS_ORDINAL_DLL", false);
}
