use ring3_core::{
    FileOffset, PeDelayImportDescriptor, PeDelayImportError, PeDelayImportTable, PeHeaderError,
    PeKind, PeRvaError, RelativeVirtualAddress, parse_pe_delay_import_descriptors,
};

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn fixed(plus: bool) -> usize {
    if plus { 112 } else { 96 }
}

fn directory(bytes: &mut [u8], plus: bool, rva: u32, size: u32) {
    put32(bytes, 152 + fixed(plus) + 104, rva);
    put32(bytes, 152 + fixed(plus) + 108, size);
}

fn section(bytes: &mut [u8], plus: bool, index: usize, fields: [u32; 4]) {
    let table = 152 + fixed(plus) + 112 + index * 40;
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
    let size = u16::try_from(fixed(plus) + 112).unwrap();
    bytes[148..150].copy_from_slice(&size.to_le_bytes());
    bytes[152..154].copy_from_slice(&if plus { 0x20b_u16 } else { 0x10b }.to_le_bytes());
    put32(&mut bytes, 212, 512);
    put32(&mut bytes, 152 + fixed(plus) - 4, 14);
    directory(&mut bytes, plus, 0x1000, 64);
    section(&mut bytes, plus, 0, [128, 0x1000, 128, 512]);
    section(&mut bytes, plus, 1, [128, 0x6000, 128, 640]);
    bytes
}

fn raw(offset: u32, words: [u32; 8]) -> PeDelayImportDescriptor {
    PeDelayImportDescriptor {
        descriptor_rva: RelativeVirtualAddress::new(0x1000 + offset),
        descriptor_file_offset: FileOffset::new(512 + u64::from(offset)),
        attributes: words[0],
        dll_name_address: words[1],
        module_handle_address: words[2],
        import_address_table_address: words[3],
        import_name_table_address: words[4],
        bound_import_address_table_address: words[5],
        unload_import_address_table_address: words[6],
        time_date_stamp: words[7],
    }
}

fn empty(plus: bool) -> PeDelayImportTable {
    PeDelayImportTable {
        kind: if plus { PeKind::Pe32Plus } else { PeKind::Pe32 },
        directory_rva: RelativeVirtualAddress::new(0x1000),
        directory_file_offset: FileOffset::new(512),
        directory_size: 64,
        descriptors: Vec::new(),
        terminator_rva: RelativeVirtualAddress::new(0x1000),
        terminator_file_offset: FileOffset::new(512),
    }
}

#[test]
fn both_widths_preserve_raw_words_order_coordinates_and_input() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        directory(&mut bytes, plus, 0x1000, 96);
        for (target, value) in bytes[512..576].iter_mut().zip(1..) {
            *target = value;
        }
        let before = bytes.clone();
        let result = parse_pe_delay_import_descriptors(&bytes);
        assert_eq!(bytes, before);
        drop(bytes);
        assert_eq!(
            result,
            Ok(Some(PeDelayImportTable {
                directory_size: 96,
                descriptors: vec![
                    raw(
                        0,
                        [
                            0x0403_0201,
                            0x0807_0605,
                            0x0c0b_0a09,
                            0x100f_0e0d,
                            0x1413_1211,
                            0x1817_1615,
                            0x1c1b_1a19,
                            0x201f_1e1d
                        ]
                    ),
                    raw(
                        32,
                        [
                            0x2423_2221,
                            0x2827_2625,
                            0x2c2b_2a29,
                            0x302f_2e2d,
                            0x3433_3231,
                            0x3837_3635,
                            0x3c3b_3a39,
                            0x403f_3e3d
                        ]
                    ),
                ],
                terminator_rva: RelativeVirtualAddress::new(0x1040),
                terminator_file_offset: FileOffset::new(576),
                ..empty(plus)
            }))
        );
    }
}

#[test]
fn every_nonzero_word_prevents_termination_without_target_reads() {
    for plus in [false, true] {
        for index in 0..8 {
            for value in [1, u32::MAX] {
                let mut bytes = fixture(plus);
                put32(&mut bytes, 512 + index * 4, value);
                let mut words = [0; 8];
                words[index] = value;
                assert_eq!(
                    parse_pe_delay_import_descriptors(&bytes),
                    Ok(Some(PeDelayImportTable {
                        descriptors: vec![raw(0, words)],
                        terminator_rva: RelativeVirtualAddress::new(0x1020),
                        terminator_file_offset: FileOffset::new(544),
                        ..empty(plus)
                    }))
                );
            }
        }
        for attributes in [0, 1, 2, 3, u32::MAX] {
            let mut bytes = fixture(plus);
            bytes[512..544].fill(0xff);
            put32(&mut bytes, 512, attributes);
            let mut words = [u32::MAX; 8];
            words[0] = attributes;
            assert_eq!(
                parse_pe_delay_import_descriptors(&bytes)
                    .unwrap()
                    .unwrap()
                    .descriptors,
                vec![raw(0, words)]
            );
        }
    }
}

#[test]
fn absent_slots_and_present_empty_table_are_distinct() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        assert_eq!(
            parse_pe_delay_import_descriptors(&bytes),
            Ok(Some(empty(plus)))
        );
        directory(&mut bytes, plus, 0, 0);
        assert_eq!(parse_pe_delay_import_descriptors(&bytes), Ok(None));
        directory(&mut bytes, plus, u32::MAX, u32::MAX);
        put32(&mut bytes, 152 + fixed(plus) - 4, 13);
        assert_eq!(parse_pe_delay_import_descriptors(&bytes), Ok(None));
    }
}

#[test]
fn consistency_and_declared_end_precede_record_checks() {
    for plus in [false, true] {
        for (rva, size) in [(0, u32::MAX), (u32::MAX, 0)] {
            let mut bytes = fixture(plus);
            directory(&mut bytes, plus, rva, size);
            assert_eq!(
                parse_pe_delay_import_descriptors(&bytes),
                Err(PeDelayImportError::InconsistentDirectory {
                    rva: RelativeVirtualAddress::new(rva),
                    size,
                })
            );
        }
        let mut bytes = fixture(plus);
        directory(&mut bytes, plus, u32::MAX, 2);
        assert_eq!(
            parse_pe_delay_import_descriptors(&bytes),
            Err(PeDelayImportError::DirectoryRangeOverflow {
                rva: RelativeVirtualAddress::new(u32::MAX),
                size: 2,
            })
        );
        directory(&mut bytes, plus, u32::MAX, 1);
        assert_eq!(
            parse_pe_delay_import_descriptors(&bytes),
            Err(PeDelayImportError::TruncatedDescriptor {
                descriptor_index: 0,
                remaining: 1,
            })
        );
    }
}

#[test]
fn every_short_record_and_missing_terminator_has_exact_index() {
    for plus in [false, true] {
        for index in 0..=2_u16 {
            for remaining in 0..32 {
                if index == 0 && remaining == 0 {
                    continue;
                }
                let mut bytes = fixture(plus);
                let offset = usize::from(index) * 32;
                bytes[512..512 + offset].fill(0xff);
                directory(&mut bytes, plus, 0x1000, u32::from(index) * 32 + remaining);
                let error = if remaining == 0 {
                    PeDelayImportError::MissingTerminator {
                        descriptor_index: index,
                    }
                } else {
                    PeDelayImportError::TruncatedDescriptor {
                        descriptor_index: index,
                        remaining,
                    }
                };
                assert_eq!(parse_pe_delay_import_descriptors(&bytes), Err(error));
            }
        }
    }
}

#[test]
fn terminator_tail_is_unread_but_declared_end_is_validated() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        section(&mut bytes, plus, 0, [32, 0x1000, 32, 512]);
        bytes[544..640].fill(0xff);
        let size = u32::MAX - 0x1000 + 1;
        directory(&mut bytes, plus, 0x1000, size);
        assert_eq!(
            parse_pe_delay_import_descriptors(&bytes),
            Ok(Some(PeDelayImportTable {
                directory_size: size,
                ..empty(plus)
            }))
        );
        directory(&mut bytes, plus, 0x1000, size + 1);
        assert!(matches!(
            parse_pe_delay_import_descriptors(&bytes),
            Err(PeDelayImportError::DirectoryRangeOverflow { .. })
        ));
    }
}

#[test]
fn high_endpoint_unaligned_and_header_backed_records_are_readable() {
    for plus in [false, true] {
        for rva in [u32::MAX - 31, 0x1001] {
            let mut bytes = fixture(plus);
            section(&mut bytes, plus, 0, [32, rva, 32, 512]);
            directory(&mut bytes, plus, rva, 32);
            assert_eq!(
                parse_pe_delay_import_descriptors(&bytes),
                Ok(Some(PeDelayImportTable {
                    directory_rva: RelativeVirtualAddress::new(rva),
                    directory_size: 32,
                    terminator_rva: RelativeVirtualAddress::new(rva),
                    ..empty(plus)
                }))
            );
        }
        let mut bytes = fixture(plus);
        directory(&mut bytes, plus, 464, 32);
        assert_eq!(
            parse_pe_delay_import_descriptors(&bytes),
            Ok(Some(PeDelayImportTable {
                directory_rva: RelativeVirtualAddress::new(464),
                directory_file_offset: FileOffset::new(464),
                directory_size: 32,
                terminator_rva: RelativeVirtualAddress::new(464),
                terminator_file_offset: FileOffset::new(464),
                ..empty(plus)
            }))
        );
    }
}

#[test]
fn every_consumed_prefix_preserves_conservative_mapping_errors() {
    for plus in [false, true] {
        let start = RelativeVirtualAddress::new(0x1000);
        let length = 64;
        let cases = [
            (
                [32, 0x1000, 32, 512],
                [128, 0x1020, 128, 640],
                PeRvaError::CrossesRegionBoundary { start, length },
            ),
            (
                [128, 0x1000, 128, 512],
                [128, 0x1020, 128, 640],
                PeRvaError::AmbiguousRange { start, length },
            ),
            (
                [128, 0x1000, 32, 512],
                [128, 0x6000, 128, 640],
                PeRvaError::NotFileBacked {
                    start,
                    length,
                    section_index: 0,
                },
            ),
            (
                [32, 0x1000, 128, 512],
                [128, 0x6000, 128, 640],
                PeRvaError::RawPaddingUnsupported {
                    start,
                    length,
                    section_index: 0,
                },
            ),
        ];
        for (first, second, cause) in cases {
            let mut bytes = fixture(plus);
            bytes[512..544].fill(0xff);
            section(&mut bytes, plus, 0, first);
            section(&mut bytes, plus, 1, second);
            assert_eq!(
                parse_pe_delay_import_descriptors(&bytes),
                Err(PeDelayImportError::DescriptorRange {
                    descriptor_index: 1,
                    start,
                    length,
                    cause,
                })
            );
        }
        for (fields, cause) in [
            (
                [128, 0x3000, 128, 512],
                PeRvaError::UnmappedRva { start, length: 32 },
            ),
            (
                [0, 0x1000, 128, 512],
                PeRvaError::ZeroVirtualSizeUnsupported {
                    start,
                    length: 32,
                    section_index: 0,
                },
            ),
        ] {
            let mut bytes = fixture(plus);
            section(&mut bytes, plus, 0, fields);
            assert_eq!(
                parse_pe_delay_import_descriptors(&bytes),
                Err(PeDelayImportError::DescriptorRange {
                    descriptor_index: 0,
                    start,
                    length: 32,
                    cause,
                })
            );
        }
    }
}

#[test]
fn descriptor_limit_allows_exact_terminator_and_preserves_error_order() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        bytes.resize(4768, 0);
        section(&mut bytes, plus, 0, [4128, 0x1000, 4128, 512]);
        section(&mut bytes, plus, 1, [128, 0x6000, 128, 4640]);
        bytes[512..4608].fill(0xff);
        directory(&mut bytes, plus, 0x1000, 4128);
        let table = parse_pe_delay_import_descriptors(&bytes).unwrap().unwrap();
        assert_eq!(table.descriptors.len(), 128);
        assert_eq!(table.descriptors[127], raw(4064, [u32::MAX; 8]));
        assert_eq!(table.terminator_rva, RelativeVirtualAddress::new(0x2000));
        assert_eq!(table.terminator_file_offset, FileOffset::new(4608));
        put32(&mut bytes, 4608, 1);
        assert_eq!(
            parse_pe_delay_import_descriptors(&bytes),
            Err(PeDelayImportError::DescriptorLimitExceeded {
                descriptor_index: 128,
                limit: 128,
            })
        );
        directory(&mut bytes, plus, 0x1000, 4096);
        assert_eq!(
            parse_pe_delay_import_descriptors(&bytes),
            Err(PeDelayImportError::MissingTerminator {
                descriptor_index: 128
            })
        );
        directory(&mut bytes, plus, 0x1000, 4127);
        assert_eq!(
            parse_pe_delay_import_descriptors(&bytes),
            Err(PeDelayImportError::TruncatedDescriptor {
                descriptor_index: 128,
                remaining: 31
            })
        );
        directory(&mut bytes, plus, 0x1000, 4128);
        section(&mut bytes, plus, 0, [4128, 0x1000, 4096, 512]);
        let start = RelativeVirtualAddress::new(0x1000);
        assert_eq!(
            parse_pe_delay_import_descriptors(&bytes),
            Err(PeDelayImportError::DescriptorRange {
                descriptor_index: 128,
                start,
                length: 4128,
                cause: PeRvaError::NotFileBacked {
                    start,
                    length: 4128,
                    section_index: 0
                },
            })
        );
    }
}

#[test]
fn invalid_base_precedes_absence_and_directory_failures() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        directory(&mut bytes, plus, 0, 0);
        section(&mut bytes, plus, 1, [128, 0x6000, 129, 640]);
        assert!(matches!(
            parse_pe_delay_import_descriptors(&bytes),
            Err(PeDelayImportError::Base(PeRvaError::Parse(
                PeHeaderError::SectionRawDataOutOfBounds {
                    section_index: 1,
                    ..
                }
            )))
        ));
        bytes[0] = 0;
        assert_eq!(
            parse_pe_delay_import_descriptors(&bytes),
            Err(PeDelayImportError::Base(PeRvaError::Parse(
                PeHeaderError::InvalidDosSignature {
                    offset: FileOffset::new(0)
                }
            )))
        );
    }
}

fn generated_delay_fixture(variable: &str, plus: bool) {
    let path = std::env::var_os(variable).expect("explicit generated delay fixture path");
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(bytes.len(), if plus { 3072 } else { 2560 });
    let (rva, offset, words) = if plus {
        (8200, 1544, [1, 8288, 12288, 12296, 8264, 0, 0, 0])
    } else {
        (8192, 1536, [1, 8276, 12288, 12296, 8256, 0, 0, 0])
    };
    assert_eq!(
        parse_pe_delay_import_descriptors(&bytes),
        Ok(Some(PeDelayImportTable {
            kind: if plus { PeKind::Pe32Plus } else { PeKind::Pe32 },
            directory_rva: RelativeVirtualAddress::new(rva),
            directory_file_offset: FileOffset::new(offset),
            directory_size: 64,
            descriptors: vec![PeDelayImportDescriptor {
                descriptor_rva: RelativeVirtualAddress::new(rva),
                descriptor_file_offset: FileOffset::new(offset),
                ..raw(0, words)
            }],
            terminator_rva: RelativeVirtualAddress::new(rva + 32),
            terminator_file_offset: FileOffset::new(offset + 32),
        }))
    );
}

#[test]
#[ignore = "requires an explicit generated delay fixture path"]
fn generated_pe32_delay_descriptors_match_raw_metadata() {
    generated_delay_fixture("RING3_DELAY_PE32_FIXTURE", false);
}

#[test]
#[ignore = "requires an explicit generated delay fixture path"]
fn generated_pe32plus_delay_descriptors_match_raw_metadata() {
    generated_delay_fixture("RING3_DELAY_PE32PLUS_FIXTURE", true);
}
