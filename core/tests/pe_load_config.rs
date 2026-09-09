use ring3_core::{
    FileOffset, PeHeaderError, PeKind, PeLoadConfigError, PeLoadConfigPrefix, PeRvaError,
    RelativeVirtualAddress, parse_pe_load_config_prefix,
};

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn fixed(plus: bool) -> usize {
    if plus { 112 } else { 96 }
}

fn directory(bytes: &mut [u8], plus: bool, rva: u32, size: u32) {
    put32(bytes, 152 + fixed(plus) + 80, rva);
    put32(bytes, 152 + fixed(plus) + 84, size);
}

fn record(bytes: &mut [u8], offset: usize, structure_size: u32) {
    for (field, value) in [0, 4, 12, 16, 20].into_iter().zip([
        structure_size,
        0xaabb_ccdd,
        0x1122_3344,
        0x5566_7788,
        u32::MAX,
    ]) {
        put32(bytes, offset + field, value);
    }
    bytes[offset + 8..offset + 12].copy_from_slice(&[0x34, 0x12, 0xcd, 0xab]);
}

fn fixture(plus: bool) -> Vec<u8> {
    let mut bytes = vec![0; 768];
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 60, 128);
    bytes[128..132].copy_from_slice(b"PE\0\0");
    bytes[134..136].copy_from_slice(&1_u16.to_le_bytes());
    bytes[148..150].copy_from_slice(&u16::try_from(fixed(plus) + 88).unwrap().to_le_bytes());
    bytes[152..154].copy_from_slice(&if plus { 0x20b_u16 } else { 0x10b }.to_le_bytes());
    put32(&mut bytes, 212, 512);
    put32(&mut bytes, 152 + fixed(plus) - 4, 11);
    for (field, value) in [8, 12, 16, 20].into_iter().zip([256, 0x1000, 256, 512]) {
        put32(&mut bytes, 152 + fixed(plus) + 88 + field, value);
    }
    directory(&mut bytes, plus, 0x1000, 24);
    record(&mut bytes, 512, if plus { 112 } else { 64 });
    bytes
}

fn expected(
    plus: bool,
    rva: u32,
    offset: u64,
    size: u32,
    structure_size: u32,
) -> PeLoadConfigPrefix {
    PeLoadConfigPrefix {
        kind: if plus { PeKind::Pe32Plus } else { PeKind::Pe32 },
        directory_rva: RelativeVirtualAddress::new(rva),
        directory_file_offset: FileOffset::new(offset),
        directory_size: size,
        structure_size,
        time_date_stamp: 0xaabb_ccdd,
        major_version: 0x1234,
        minor_version: 0xabcd,
        global_flags_clear: 0x1122_3344,
        global_flags_set: 0x5566_7788,
        critical_section_default_timeout: u32::MAX,
    }
}

#[test]
fn both_widths_own_exact_prefix_metadata_after_input_is_dropped() {
    for plus in [false, true] {
        let prefix = {
            let bytes = fixture(plus);
            let before = bytes.clone();
            let result = parse_pe_load_config_prefix(&bytes).unwrap().unwrap();
            assert_eq!(bytes, before);
            result
        };
        assert_eq!(
            prefix,
            expected(plus, 0x1000, 512, 24, if plus { 112 } else { 64 })
        );
    }
}

#[test]
fn missing_or_zero_slots_are_absent_but_zero_structure_size_is_unsupported() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        put32(&mut bytes, 512, 0);
        assert_eq!(
            parse_pe_load_config_prefix(&bytes),
            Err(PeLoadConfigError::UnsupportedStructureSize {
                structure_size: 0,
                required: 24
            })
        );
        directory(&mut bytes, plus, 0, 0);
        assert_eq!(parse_pe_load_config_prefix(&bytes), Ok(None));
        directory(&mut bytes, plus, u32::MAX, u32::MAX);
        put32(&mut bytes, 152 + fixed(plus) - 4, 10);
        assert_eq!(parse_pe_load_config_prefix(&bytes), Ok(None));
    }
}

#[test]
fn base_validation_precedes_absence_and_slot_errors() {
    for plus in [false, true] {
        for (rva, size) in [(0, 0), (0, 1), (1, 0), (u32::MAX, 24), (0x1000, 24)] {
            let mut bytes = fixture(plus);
            directory(&mut bytes, plus, rva, size);
            bytes[0] = 0;
            assert_eq!(
                parse_pe_load_config_prefix(&bytes),
                Err(PeLoadConfigError::Base(PeRvaError::Parse(
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
                parse_pe_load_config_prefix(&bytes),
                Err(PeLoadConfigError::InconsistentDirectory {
                    rva: RelativeVirtualAddress::new(rva),
                    size
                })
            );
        }
        directory(&mut bytes, plus, u32::MAX, 2);
        assert_eq!(
            parse_pe_load_config_prefix(&bytes),
            Err(PeLoadConfigError::DirectoryRangeOverflow {
                rva: RelativeVirtualAddress::new(u32::MAX),
                size: 2
            })
        );
        for size in 1..24 {
            directory(&mut bytes, plus, 0x3000, size);
            assert_eq!(
                parse_pe_load_config_prefix(&bytes),
                Err(PeLoadConfigError::TruncatedDirectory {
                    rva: RelativeVirtualAddress::new(0x3000),
                    size,
                    required: 24
                })
            );
        }
    }
}

#[test]
fn only_the_supported_prefix_is_read_and_sizes_remain_independent() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        bytes.truncate(536);
        for field in [8, 16] {
            put32(&mut bytes, 152 + fixed(plus) + 88 + field, 24);
        }
        for (size, structure_size) in [(24, 64), (512, 24), (0xffff_f000, u32::MAX)] {
            directory(&mut bytes, plus, 0x1000, size);
            record(&mut bytes, 512, structure_size);
            assert_eq!(
                parse_pe_load_config_prefix(&bytes),
                Ok(Some(expected(plus, 0x1000, 512, size, structure_size)))
            );
        }
    }
}

#[test]
fn physical_prefix_errors_precede_embedded_size_validation() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        put32(&mut bytes, 512, 0);
        for (rva, cause) in [
            (
                0x3000,
                PeRvaError::UnmappedRva {
                    start: RelativeVirtualAddress::new(0x3000),
                    length: 24,
                },
            ),
            (
                0x10f0,
                PeRvaError::CrossesRegionBoundary {
                    start: RelativeVirtualAddress::new(0x10f0),
                    length: 24,
                },
            ),
        ] {
            directory(&mut bytes, plus, rva, 512);
            assert_eq!(
                parse_pe_load_config_prefix(&bytes),
                Err(PeLoadConfigError::PrefixRange {
                    start: RelativeVirtualAddress::new(rva),
                    length: 24,
                    cause
                })
            );
        }
        directory(&mut bytes, plus, 0x1000, 24);
        put32(&mut bytes, 152 + fixed(plus) + 88 + 16, 23);
        assert_eq!(
            parse_pe_load_config_prefix(&bytes),
            Err(PeLoadConfigError::PrefixRange {
                start: RelativeVirtualAddress::new(0x1000),
                length: 24,
                cause: PeRvaError::NotFileBacked {
                    start: RelativeVirtualAddress::new(0x1000),
                    length: 24,
                    section_index: 0
                },
            })
        );
    }
}

#[test]
fn each_small_embedded_size_is_rejected_after_a_complete_prefix() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        for structure_size in 0..24 {
            put32(&mut bytes, 512, structure_size);
            assert_eq!(
                parse_pe_load_config_prefix(&bytes),
                Err(PeLoadConfigError::UnsupportedStructureSize {
                    structure_size,
                    required: 24
                })
            );
        }
        record(&mut bytes, 449, 24);
        directory(&mut bytes, plus, 449, 24);
        assert_eq!(
            parse_pe_load_config_prefix(&bytes),
            Ok(Some(expected(plus, 449, 449, 24, 24)))
        );
    }
}

#[test]
fn prefix_ending_exactly_at_u32_boundary_does_not_wrap() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        put32(&mut bytes, 152 + fixed(plus) + 88 + 12, 0xffff_ff00);
        directory(&mut bytes, plus, 0xffff_ffe8, 24);
        record(&mut bytes, 744, 24);
        assert_eq!(
            parse_pe_load_config_prefix(&bytes),
            Ok(Some(expected(plus, 0xffff_ffe8, 744, 24, 24)))
        );
        directory(&mut bytes, plus, 0xffff_ffe8, 25);
        assert_eq!(
            parse_pe_load_config_prefix(&bytes),
            Err(PeLoadConfigError::DirectoryRangeOverflow {
                rva: RelativeVirtualAddress::new(0xffff_ffe8),
                size: 25
            })
        );
    }
}
