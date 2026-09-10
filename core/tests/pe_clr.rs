use ring3_core::{
    FileOffset, PeClrDataDirectory, PeClrError, PeClrHeader, PeHeaderError, PeKind, PeRvaError,
    RelativeVirtualAddress, parse_pe_clr_header,
};

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn fixed(plus: bool) -> usize {
    if plus { 112 } else { 96 }
}

fn section(plus: bool) -> usize {
    184 + fixed(plus) + 120
}

fn directory(bytes: &mut [u8], plus: bool, rva: u32, size: u32) {
    put32(bytes, 184 + fixed(plus) + 112, rva);
    put32(bytes, 184 + fixed(plus) + 116, size);
}

fn record(bytes: &mut [u8], file: usize, cb: u32) {
    put32(bytes, file, cb);
    bytes[file + 4..file + 8].copy_from_slice(&[2, 0, 5, 0]);
    put32(bytes, file + 16, 1);
    put32(bytes, file + 20, 0x0600_0001);
    for (index, offset) in (0_u32..).zip([8, 24, 32, 40, 48, 56, 64]) {
        put32(bytes, file + offset, 0xf000_0000 + index);
        put32(bytes, file + offset + 4, 0x1020_3040 + index);
    }
}

fn fixture(plus: bool) -> Vec<u8> {
    let mut bytes = vec![0; 1536];
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 60, 160);
    bytes[160..164].copy_from_slice(b"PE\0\0");
    bytes[166..168].copy_from_slice(&1_u16.to_le_bytes());
    bytes[180..182].copy_from_slice(&u16::try_from(fixed(plus) + 120).unwrap().to_le_bytes());
    bytes[184..186].copy_from_slice(&if plus { 0x20b_u16 } else { 0x10b }.to_le_bytes());
    put32(&mut bytes, 244, 512);
    put32(&mut bytes, 184 + fixed(plus) - 4, 15);
    for (offset, value) in [8, 12, 16, 20].into_iter().zip([512, 4096, 512, 512]) {
        put32(&mut bytes, section(plus) + offset, value);
    }
    directory(&mut bytes, plus, 4160, 72);
    record(&mut bytes, 576, 72);
    bytes
}

fn expected(plus: bool, rva: u32, file: u64, size: u32, cb: u32) -> PeClrHeader {
    let pair = |index: u32| PeClrDataDirectory {
        rva: RelativeVirtualAddress::new(0xf000_0000 + index),
        size: 0x1020_3040 + index,
    };
    PeClrHeader {
        kind: if plus { PeKind::Pe32Plus } else { PeKind::Pe32 },
        directory_rva: RelativeVirtualAddress::new(rva),
        directory_file_offset: FileOffset::new(file),
        directory_size: size,
        header_size: cb,
        major_runtime_version: 2,
        minor_runtime_version: 5,
        flags: 1,
        raw_entry_point: 0x0600_0001,
        metadata: pair(0),
        resources: pair(1),
        strong_name_signature: pair(2),
        code_manager_table: pair(3),
        v_table_fixups: pair(4),
        export_address_table_jumps: pair(5),
        managed_native_header: pair(6),
    }
}

#[test]
fn both_widths_own_all_raw_fields_after_input_is_dropped() {
    for plus in [false, true] {
        let header = {
            let bytes = fixture(plus);
            let before = bytes.clone();
            let header = parse_pe_clr_header(&bytes).unwrap().unwrap();
            assert_eq!(Some(header), parse_pe_clr_header(&bytes).unwrap());
            assert_eq!(bytes, before);
            header
        };
        assert_eq!(header, expected(plus, 4160, 576, 72, 72));
    }
}

#[test]
fn entry_word_flags_and_versions_stay_raw() {
    for plus in [false, true] {
        for flags in [0, 1, 16, 17, 0x8000_0000, u32::MAX] {
            for raw_entry_point in [0, 0x0600_0001, u32::MAX] {
                let mut bytes = fixture(plus);
                put32(&mut bytes, 592, flags);
                put32(&mut bytes, 596, raw_entry_point);
                bytes[580..584].fill(0xff);
                let mut wanted = expected(plus, 4160, 576, 72, 72);
                wanted.flags = flags;
                wanted.raw_entry_point = raw_entry_point;
                wanted.major_runtime_version = u16::MAX;
                wanted.minor_runtime_version = u16::MAX;
                assert_eq!(parse_pe_clr_header(&bytes), Ok(Some(wanted)));
            }
        }
    }
}

#[test]
fn absent_slots_do_not_read_a_header() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        put32(&mut bytes, 576, 0);
        directory(&mut bytes, plus, 0, 0);
        assert_eq!(parse_pe_clr_header(&bytes), Ok(None));
        directory(&mut bytes, plus, u32::MAX, u32::MAX);
        put32(&mut bytes, 184 + fixed(plus) - 4, 14);
        assert_eq!(parse_pe_clr_header(&bytes), Ok(None));
    }
}

#[test]
fn base_validation_precedes_absence_and_directory_errors() {
    for plus in [false, true] {
        for (rva, size) in [(0, 0), (0, 1), (1, 0), (u32::MAX, 72), (4160, 72)] {
            let mut bytes = fixture(plus);
            directory(&mut bytes, plus, rva, size);
            bytes[0] = 0;
            assert_eq!(
                parse_pe_clr_header(&bytes),
                Err(PeClrError::Base(PeRvaError::Parse(
                    PeHeaderError::InvalidDosSignature {
                        offset: FileOffset::new(0)
                    }
                )))
            );
        }
    }
}

#[test]
fn consistency_coordinate_end_and_directory_minimum_precede_backing() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        for (rva, size) in [(0, u32::MAX), (u32::MAX, 0)] {
            directory(&mut bytes, plus, rva, size);
            assert_eq!(
                parse_pe_clr_header(&bytes),
                Err(PeClrError::InconsistentDirectory {
                    rva: RelativeVirtualAddress::new(rva),
                    size
                })
            );
        }
        directory(&mut bytes, plus, u32::MAX, 2);
        assert_eq!(
            parse_pe_clr_header(&bytes),
            Err(PeClrError::DirectoryRangeOverflow {
                rva: RelativeVirtualAddress::new(u32::MAX),
                size: 2
            })
        );
        for size in 1..72 {
            directory(&mut bytes, plus, 8192, size);
            assert_eq!(
                parse_pe_clr_header(&bytes),
                Err(PeClrError::TruncatedDirectory {
                    rva: RelativeVirtualAddress::new(8192),
                    size,
                    required: 72
                })
            );
        }
    }
}

#[test]
fn only_the_fixed_prefix_is_read_and_both_sizes_remain_independent() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        bytes.truncate(648);
        for offset in [8, 16] {
            put32(&mut bytes, section(plus) + offset, 136);
        }
        for (size, cb) in [(72, u32::MAX), (1024, 72), (u32::MAX - 4159, 100)] {
            directory(&mut bytes, plus, 4160, size);
            put32(&mut bytes, 576, cb);
            assert_eq!(
                parse_pe_clr_header(&bytes),
                Ok(Some(expected(plus, 4160, 576, size, cb)))
            );
        }
    }
}

#[test]
fn conservative_prefix_ranges_precede_embedded_size() {
    for plus in [false, true] {
        for (rva, cause) in [
            (
                8192,
                PeRvaError::UnmappedRva {
                    start: RelativeVirtualAddress::new(8192),
                    length: 72,
                },
            ),
            (
                4580,
                PeRvaError::CrossesRegionBoundary {
                    start: RelativeVirtualAddress::new(4580),
                    length: 72,
                },
            ),
        ] {
            let mut bytes = fixture(plus);
            put32(&mut bytes, 576, 0);
            directory(&mut bytes, plus, rva, 72);
            assert_eq!(
                parse_pe_clr_header(&bytes),
                Err(PeClrError::PrefixRange {
                    start: RelativeVirtualAddress::new(rva),
                    length: 72,
                    cause
                })
            );
        }
        for (virtual_size, raw_size, cause) in [
            (
                0,
                512,
                PeRvaError::ZeroVirtualSizeUnsupported {
                    start: RelativeVirtualAddress::new(4160),
                    length: 72,
                    section_index: 0,
                },
            ),
            (
                100,
                512,
                PeRvaError::RawPaddingUnsupported {
                    start: RelativeVirtualAddress::new(4160),
                    length: 72,
                    section_index: 0,
                },
            ),
            (
                512,
                100,
                PeRvaError::NotFileBacked {
                    start: RelativeVirtualAddress::new(4160),
                    length: 72,
                    section_index: 0,
                },
            ),
        ] {
            let mut bytes = fixture(plus);
            put32(&mut bytes, 576, 0);
            put32(&mut bytes, section(plus) + 8, virtual_size);
            put32(&mut bytes, section(plus) + 16, raw_size);
            assert_eq!(
                parse_pe_clr_header(&bytes),
                Err(PeClrError::PrefixRange {
                    start: RelativeVirtualAddress::new(4160),
                    length: 72,
                    cause
                })
            );
        }
    }
}

#[test]
fn overlapping_regions_are_ambiguous_before_reading_cb() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        bytes[166..168].copy_from_slice(&2_u16.to_le_bytes());
        for (offset, value) in [8, 12, 16, 20].into_iter().zip([512, 4096, 512, 1024]) {
            put32(&mut bytes, section(plus) + 40 + offset, value);
        }
        assert_eq!(
            parse_pe_clr_header(&bytes),
            Err(PeClrError::PrefixRange {
                start: RelativeVirtualAddress::new(4160),
                length: 72,
                cause: PeRvaError::AmbiguousRange {
                    start: RelativeVirtualAddress::new(4160),
                    length: 72
                }
            })
        );
    }
}

#[test]
fn header_unaligned_and_last_backed_prefixes_keep_exact_coordinates() {
    for plus in [false, true] {
        for (rva, file) in [(64, 64), (4161, 577), (4536, 952)] {
            let mut bytes = fixture(plus);
            directory(&mut bytes, plus, rva, 72);
            record(&mut bytes, file, 72);
            assert_eq!(
                parse_pe_clr_header(&bytes),
                Ok(Some(expected(plus, rva, file as u64, 72, 72)))
            );
        }
    }
}

#[test]
fn short_embedded_headers_are_rejected_after_physical_prefix() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        for header_size in [0, 4, 24, 64, 71] {
            put32(&mut bytes, 576, header_size);
            assert_eq!(
                parse_pe_clr_header(&bytes),
                Err(PeClrError::UnsupportedHeaderSize {
                    header_size,
                    required: 72
                })
            );
        }
    }
}
