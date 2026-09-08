use ring3_core::{
    FileOffset, PeHeaderError, PeKind, PeRvaError, PeTlsDirectory, PeTlsDirectoryError,
    RelativeVirtualAddress, parse_pe_tls_directory,
};

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn fixed(plus: bool) -> usize {
    if plus { 112 } else { 96 }
}

fn length(plus: bool) -> u32 {
    if plus { 40 } else { 24 }
}

fn kind(plus: bool) -> PeKind {
    if plus { PeKind::Pe32Plus } else { PeKind::Pe32 }
}

fn directory(bytes: &mut [u8], plus: bool, rva: u32, size: u32) {
    put32(bytes, 152 + fixed(plus) + 72, rva);
    put32(bytes, 152 + fixed(plus) + 76, size);
}

fn section(bytes: &mut [u8], plus: bool, index: usize, fields: [u32; 4]) {
    let table = 152 + fixed(plus) + 80 + index * 40;
    for (offset, value) in [8, 12, 16, 20].into_iter().zip(fields) {
        put32(bytes, table + offset, value);
    }
}

fn fixture(plus: bool) -> Vec<u8> {
    let mut bytes = vec![0; 768];
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 60, 128);
    bytes[128..132].copy_from_slice(b"PE\0\0");
    bytes[134..136].copy_from_slice(&2_u16.to_le_bytes());
    let size = u16::try_from(fixed(plus) + 80).unwrap();
    bytes[148..150].copy_from_slice(&size.to_le_bytes());
    bytes[152..154].copy_from_slice(&if plus { 0x20b_u16 } else { 0x10b }.to_le_bytes());
    put32(&mut bytes, 212, 512);
    put32(&mut bytes, 152 + fixed(plus) - 4, 10);
    directory(&mut bytes, plus, 0x1000, length(plus));
    section(&mut bytes, plus, 0, [128, 0x1000, 128, 512]);
    section(&mut bytes, plus, 1, [128, 0x2000, 128, 640]);
    bytes
}

fn zero(plus: bool) -> PeTlsDirectory {
    PeTlsDirectory {
        kind: kind(plus),
        directory_rva: RelativeVirtualAddress::new(0x1000),
        directory_file_offset: FileOffset::new(512),
        directory_size: length(plus),
        start_address_of_raw_data: 0,
        end_address_of_raw_data: 0,
        address_of_index: 0,
        address_of_callbacks: 0,
        size_of_zero_fill: 0,
        characteristics: 0,
    }
}

#[test]
fn both_widths_decode_distinct_scalar_bytes_and_preserve_input() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        for (target, value) in bytes[512..512 + length(plus) as usize].iter_mut().zip(1..) {
            *target = value;
        }
        let before = bytes.clone();
        let expected = if plus {
            PeTlsDirectory {
                start_address_of_raw_data: 0x0807_0605_0403_0201,
                end_address_of_raw_data: 0x100f_0e0d_0c0b_0a09,
                address_of_index: 0x1817_1615_1413_1211,
                address_of_callbacks: 0x201f_1e1d_1c1b_1a19,
                size_of_zero_fill: 0x2423_2221,
                characteristics: 0x2827_2625,
                ..zero(plus)
            }
        } else {
            PeTlsDirectory {
                start_address_of_raw_data: 0x0403_0201,
                end_address_of_raw_data: 0x0807_0605,
                address_of_index: 0x0c0b_0a09,
                address_of_callbacks: 0x100f_0e0d,
                size_of_zero_fill: 0x1413_1211,
                characteristics: 0x1817_1615,
                ..zero(plus)
            }
        };
        assert_eq!(parse_pe_tls_directory(&bytes), Ok(Some(expected)));
        assert_eq!(bytes, before);
    }
}

#[test]
fn raw_maximum_reversed_and_unmapped_targets_remain_metadata() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        bytes[512..512 + length(plus) as usize].fill(0xff);
        let max = if plus { u64::MAX } else { u64::from(u32::MAX) };
        let expected = PeTlsDirectory {
            start_address_of_raw_data: max,
            end_address_of_raw_data: max,
            address_of_index: max,
            address_of_callbacks: max,
            size_of_zero_fill: u32::MAX,
            characteristics: u32::MAX,
            ..zero(plus)
        };
        assert_eq!(parse_pe_tls_directory(&bytes), Ok(Some(expected)));
        let width = if plus { 8 } else { 4 };
        bytes[512 + width..512 + width * 2].fill(0);
        assert_eq!(
            parse_pe_tls_directory(&bytes),
            Ok(Some(PeTlsDirectory {
                end_address_of_raw_data: 0,
                ..expected
            }))
        );
    }
}

#[test]
fn absent_and_zero_slots_differ_from_a_present_zero_record() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        assert_eq!(parse_pe_tls_directory(&bytes), Ok(Some(zero(plus))));
        directory(&mut bytes, plus, 0, 0);
        assert_eq!(parse_pe_tls_directory(&bytes), Ok(None));
        directory(&mut bytes, plus, u32::MAX, u32::MAX);
        put32(&mut bytes, 152 + fixed(plus) - 4, 9);
        assert_eq!(parse_pe_tls_directory(&bytes), Ok(None));
    }
}

#[test]
fn slot_consistency_precedes_declared_end_and_minimum_size() {
    for plus in [false, true] {
        for (rva, size) in [(0, u32::MAX), (u32::MAX, 0)] {
            let mut bytes = fixture(plus);
            directory(&mut bytes, plus, rva, size);
            assert_eq!(
                parse_pe_tls_directory(&bytes),
                Err(PeTlsDirectoryError::InconsistentTlsDirectory {
                    rva: RelativeVirtualAddress::new(rva),
                    size,
                })
            );
        }
        let mut bytes = fixture(plus);
        directory(&mut bytes, plus, u32::MAX, 2);
        assert_eq!(
            parse_pe_tls_directory(&bytes),
            Err(PeTlsDirectoryError::TlsDirectoryRangeOverflow {
                rva: RelativeVirtualAddress::new(u32::MAX),
                size: 2,
            })
        );
        directory(&mut bytes, plus, u32::MAX, 1);
        assert_eq!(
            parse_pe_tls_directory(&bytes),
            Err(PeTlsDirectoryError::TruncatedTlsDirectory {
                rva: RelativeVirtualAddress::new(u32::MAX),
                size: 1,
                required: length(plus),
            })
        );
    }
}

#[test]
fn every_short_width_specific_record_refuses_before_target_mapping() {
    for plus in [false, true] {
        for size in 1..length(plus) {
            let mut bytes = fixture(plus);
            directory(&mut bytes, plus, 0x9000, size);
            assert_eq!(
                parse_pe_tls_directory(&bytes),
                Err(PeTlsDirectoryError::TruncatedTlsDirectory {
                    rva: RelativeVirtualAddress::new(0x9000),
                    size,
                    required: length(plus),
                })
            );
        }
    }
}

#[test]
fn declared_tail_is_unread_but_its_coordinate_end_is_checked() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        section(
            &mut bytes,
            plus,
            0,
            [length(plus), 0x1000, length(plus), 512],
        );
        let size = u32::MAX - 0x1000 + 1;
        directory(&mut bytes, plus, 0x1000, size);
        assert_eq!(
            parse_pe_tls_directory(&bytes),
            Ok(Some(PeTlsDirectory {
                directory_size: size,
                ..zero(plus)
            }))
        );
        directory(&mut bytes, plus, 0x1000, size + 1);
        assert_eq!(
            parse_pe_tls_directory(&bytes),
            Err(PeTlsDirectoryError::TlsDirectoryRangeOverflow {
                rva: RelativeVirtualAddress::new(0x1000),
                size: size + 1,
            })
        );
    }
}

#[test]
fn exact_top_of_rva_space_and_unaligned_records_are_readable() {
    for plus in [false, true] {
        for rva in [u32::MAX - length(plus) + 1, 0x1001] {
            let mut bytes = fixture(plus);
            section(&mut bytes, plus, 0, [length(plus), rva, length(plus), 512]);
            directory(&mut bytes, plus, rva, length(plus));
            assert_eq!(
                parse_pe_tls_directory(&bytes),
                Ok(Some(PeTlsDirectory {
                    directory_rva: RelativeVirtualAddress::new(rva),
                    ..zero(plus)
                }))
            );
        }
    }
}

#[test]
fn fixed_prefix_can_be_header_backed() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        directory(&mut bytes, plus, 464, length(plus));
        assert_eq!(
            parse_pe_tls_directory(&bytes),
            Ok(Some(PeTlsDirectory {
                directory_rva: RelativeVirtualAddress::new(464),
                directory_file_offset: FileOffset::new(464),
                ..zero(plus)
            }))
        );
    }
}

#[test]
fn conservative_prefix_mapping_preserves_each_refusal_cause() {
    for plus in [false, true] {
        let start = RelativeVirtualAddress::new(0x1000);
        let length = length(plus);
        let cases = [
            (
                [128, 0x3000, 128, 512],
                [128, 0x2000, 128, 640],
                PeRvaError::UnmappedRva { start, length },
            ),
            (
                [length - 1, 0x1000, length - 1, 512],
                [128, 0x1000 + length - 1, 128, 640],
                PeRvaError::CrossesRegionBoundary { start, length },
            ),
            (
                [128, 0x1000, 128, 512],
                [128, 0x1001, 128, 640],
                PeRvaError::AmbiguousRange { start, length },
            ),
            (
                [128, 0x1000, length - 1, 512],
                [128, 0x2000, 128, 640],
                PeRvaError::NotFileBacked {
                    start,
                    length,
                    section_index: 0,
                },
            ),
            (
                [length - 1, 0x1000, 128, 512],
                [128, 0x2000, 128, 640],
                PeRvaError::RawPaddingUnsupported {
                    start,
                    length,
                    section_index: 0,
                },
            ),
            (
                [0, 0x1000, 128, 512],
                [128, 0x2000, 128, 640],
                PeRvaError::ZeroVirtualSizeUnsupported {
                    start,
                    length,
                    section_index: 0,
                },
            ),
        ];
        for (first, second, cause) in cases {
            let mut bytes = fixture(plus);
            section(&mut bytes, plus, 0, first);
            section(&mut bytes, plus, 1, second);
            assert_eq!(
                parse_pe_tls_directory(&bytes),
                Err(PeTlsDirectoryError::DirectoryRange {
                    start,
                    length,
                    cause
                })
            );
        }
    }
}

#[test]
fn base_failures_precede_absence_and_directory_errors() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        directory(&mut bytes, plus, 0, 0);
        section(&mut bytes, plus, 1, [128, 0x2000, 129, 640]);
        assert!(matches!(
            parse_pe_tls_directory(&bytes),
            Err(PeTlsDirectoryError::Base(PeRvaError::Parse(
                PeHeaderError::SectionRawDataOutOfBounds {
                    section_index: 1,
                    ..
                }
            )))
        ));
        bytes[0] = 0;
        assert_eq!(
            parse_pe_tls_directory(&bytes),
            Err(PeTlsDirectoryError::Base(PeRvaError::Parse(
                PeHeaderError::InvalidDosSignature {
                    offset: FileOffset::new(0)
                }
            )))
        );
    }
}
