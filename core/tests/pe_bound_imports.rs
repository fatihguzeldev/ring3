use ring3_core::{
    FileOffset, PeBoundForwarderRef, PeBoundImportDescriptor, PeBoundImportError,
    PeBoundImportTable, PeKind, PeRvaError, RelativeVirtualAddress,
    parse_pe_bound_import_descriptors,
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

fn empty(plus: bool, size: u32) -> PeBoundImportTable {
    PeBoundImportTable {
        kind: if plus { PeKind::Pe32Plus } else { PeKind::Pe32 },
        directory_rva: RelativeVirtualAddress::new(4096),
        directory_file_offset: FileOffset::new(512),
        directory_size: size,
        descriptors: Vec::new(),
        terminator_rva: RelativeVirtualAddress::new(4096),
        terminator_file_offset: FileOffset::new(512),
    }
}

#[test]
fn raw_interleaved_records_keep_zero_refs_reserved_words_and_owned_coordinates() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 48);
        record(&mut bytes, 512, u32::MAX, u16::MAX, 2);
        record(&mut bytes, 528, 0x1234_5678, 1, u16::MAX);
        record(&mut bytes, 536, 0, 0, 1);
        record(&mut bytes, 544, 1, u16::MAX, 7);
        let before = bytes.clone();
        let result = parse_pe_bound_import_descriptors(&bytes);
        assert_eq!(result, parse_pe_bound_import_descriptors(&bytes));
        assert_eq!(bytes, before);
        bytes.fill(0);
        drop(bytes);
        assert_eq!(
            result,
            Ok(Some(PeBoundImportTable {
                descriptors: vec![
                    PeBoundImportDescriptor {
                        descriptor_rva: RelativeVirtualAddress::new(4096),
                        descriptor_file_offset: FileOffset::new(512),
                        time_date_stamp: u32::MAX,
                        module_name_offset: u16::MAX,
                        number_of_module_forwarder_refs: 2,
                        forwarder_refs: vec![
                            PeBoundForwarderRef {
                                reference_rva: RelativeVirtualAddress::new(4104),
                                reference_file_offset: FileOffset::new(520),
                                time_date_stamp: 0,
                                module_name_offset: 0,
                                reserved: 0,
                            },
                            PeBoundForwarderRef {
                                reference_rva: RelativeVirtualAddress::new(4112),
                                reference_file_offset: FileOffset::new(528),
                                time_date_stamp: 0x1234_5678,
                                module_name_offset: 1,
                                reserved: u16::MAX,
                            },
                        ],
                    },
                    PeBoundImportDescriptor {
                        descriptor_rva: RelativeVirtualAddress::new(4120),
                        descriptor_file_offset: FileOffset::new(536),
                        time_date_stamp: 0,
                        module_name_offset: 0,
                        number_of_module_forwarder_refs: 1,
                        forwarder_refs: vec![PeBoundForwarderRef {
                            reference_rva: RelativeVirtualAddress::new(4128),
                            reference_file_offset: FileOffset::new(544),
                            time_date_stamp: 1,
                            module_name_offset: u16::MAX,
                            reserved: 7,
                        }],
                    },
                ],
                terminator_rva: RelativeVirtualAddress::new(4136),
                terminator_file_offset: FileOffset::new(552),
                ..empty(plus, 48)
            }))
        );
    }
}

#[test]
fn base_absence_and_present_empty_remain_distinct() {
    assert!(matches!(
        parse_pe_bound_import_descriptors(&[]),
        Err(PeBoundImportError::Base(_))
    ));
    for plus in [false, true] {
        let mut bytes = fixture(plus, 8);
        assert_eq!(
            parse_pe_bound_import_descriptors(&bytes),
            Ok(Some(empty(plus, 8)))
        );
        directory(&mut bytes, plus, 0, 0);
        assert_eq!(parse_pe_bound_import_descriptors(&bytes), Ok(None));
        directory(&mut bytes, plus, u32::MAX, u32::MAX);
        put32(&mut bytes, 152 + fixed(plus) - 4, 11);
        assert_eq!(parse_pe_bound_import_descriptors(&bytes), Ok(None));
        bytes[0] = 0;
        assert!(matches!(
            parse_pe_bound_import_descriptors(&bytes),
            Err(PeBoundImportError::Base(_))
        ));
    }
}

#[test]
fn consistency_and_coordinate_end_precede_target_reads() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 8);
        for (rva, size) in [(0, 8), (4096, 0)] {
            directory(&mut bytes, plus, rva, size);
            assert_eq!(
                parse_pe_bound_import_descriptors(&bytes),
                Err(PeBoundImportError::InconsistentDirectory {
                    rva: RelativeVirtualAddress::new(rva),
                    size
                })
            );
        }
        directory(&mut bytes, plus, 0xffff_fff8, 9);
        assert_eq!(
            parse_pe_bound_import_descriptors(&bytes),
            Err(PeBoundImportError::DirectoryRangeOverflow {
                rva: RelativeVirtualAddress::new(0xffff_fff8),
                size: 9
            })
        );
        directory(&mut bytes, plus, 0xffff_fff8, 8);
        section(&mut bytes, plus, 0, [8, 0xffff_fff8, 8, 512]);
        let mut expected = empty(plus, 8);
        expected.directory_rva = RelativeVirtualAddress::new(0xffff_fff8);
        expected.terminator_rva = expected.directory_rva;
        assert_eq!(
            parse_pe_bound_import_descriptors(&bytes),
            Ok(Some(expected))
        );
    }
}

#[test]
fn header_table_and_ignored_declared_tail_need_only_consumed_prefix() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 0x10_0000);
        assert_eq!(
            parse_pe_bound_import_descriptors(&bytes),
            Ok(Some(empty(plus, 0x10_0000)))
        );
        directory(&mut bytes, plus, 480, 8);
        let mut expected = empty(plus, 8);
        expected.directory_rva = RelativeVirtualAddress::new(480);
        expected.terminator_rva = expected.directory_rva;
        expected.directory_file_offset = FileOffset::new(480);
        expected.terminator_file_offset = expected.directory_file_offset;
        assert_eq!(
            parse_pe_bound_import_descriptors(&bytes),
            Ok(Some(expected))
        );
    }
}

#[test]
fn descriptor_remaining_precedes_mapping_and_count_cap() {
    for plus in [false, true] {
        for index in [0_u16, 1, 128] {
            for remaining in 0..8 {
                if index == 0 && remaining == 0 {
                    continue;
                }
                let mut bytes = fixture(plus, u32::from(index) * 8 + remaining);
                for i in 0..index {
                    record(&mut bytes, 512 + usize::from(i) * 8, 1, 0, 0);
                }
                let expected = if remaining == 0 {
                    PeBoundImportError::MissingTerminator {
                        descriptor_index: index,
                    }
                } else {
                    PeBoundImportError::TruncatedDescriptor {
                        descriptor_index: index,
                        remaining,
                    }
                };
                assert_eq!(parse_pe_bound_import_descriptors(&bytes), Err(expected));
            }
        }
        let mut bytes = fixture(plus, 7);
        directory(&mut bytes, plus, 0x8000, 7);
        assert_eq!(
            parse_pe_bound_import_descriptors(&bytes),
            Err(PeBoundImportError::TruncatedDescriptor {
                descriptor_index: 0,
                remaining: 7
            })
        );
    }
}

#[test]
fn descriptor_sentinel_wins_at_limit_and_nonzero_record_refuses_before_refs() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 1032);
        for index in 0..128 {
            record(&mut bytes, 512 + index * 8, 1, 0, 0);
        }
        let table = parse_pe_bound_import_descriptors(&bytes).unwrap().unwrap();
        assert_eq!(table.descriptors.len(), 128);
        assert_eq!(table.terminator_file_offset, FileOffset::new(1536));
        record(&mut bytes, 1536, 0, 0, u16::MAX);
        assert_eq!(
            parse_pe_bound_import_descriptors(&bytes),
            Err(PeBoundImportError::DescriptorLimitExceeded {
                descriptor_index: 128,
                limit: 128
            })
        );
        section(&mut bytes, plus, 0, [1024, 4096, 1024, 512]);
        assert_eq!(
            parse_pe_bound_import_descriptors(&bytes),
            Err(PeBoundImportError::DescriptorRange {
                descriptor_index: 128,
                start: RelativeVirtualAddress::new(4096),
                length: 1032,
                cause: PeRvaError::CrossesRegionBoundary {
                    start: RelativeVirtualAddress::new(4096),
                    length: 1032
                }
            })
        );
    }
}

#[test]
fn aggregate_reference_cap_precedes_truncation_and_mapping() {
    for plus in [false, true] {
        for count in [1025, u16::MAX] {
            let mut bytes = fixture(plus, 8);
            section(&mut bytes, plus, 0, [8, 4096, 8, 512]);
            record(&mut bytes, 512, 1, 1, count);
            assert_eq!(
                parse_pe_bound_import_descriptors(&bytes),
                Err(PeBoundImportError::ForwarderLimitExceeded {
                    descriptor_index: 0,
                    used: 0,
                    count,
                    limit: 1024
                })
            );
        }
        let mut bytes = fixture(plus, 8224);
        record(&mut bytes, 512, 1, 1, 600);
        record(&mut bytes, 5320, 2, 2, 424);
        record(&mut bytes, 8720, 3, 3, 1);
        assert_eq!(
            parse_pe_bound_import_descriptors(&bytes),
            Err(PeBoundImportError::ForwarderLimitExceeded {
                descriptor_index: 2,
                used: 1024,
                count: 1,
                limit: 1024
            })
        );
    }
}

#[test]
fn declared_reference_group_must_fit_before_backing_is_checked() {
    for plus in [false, true] {
        for remaining in 0..16 {
            let mut bytes = fixture(plus, 8 + remaining);
            section(&mut bytes, plus, 0, [8, 4096, 8, 512]);
            record(&mut bytes, 512, 1, 1, 2);
            assert_eq!(
                parse_pe_bound_import_descriptors(&bytes),
                Err(PeBoundImportError::TruncatedForwarderRefs {
                    descriptor_index: 0,
                    count: 2,
                    remaining
                })
            );
        }
    }
}

#[test]
fn complete_prefixes_are_never_stitched_and_overlaps_remain_ambiguous() {
    for plus in [false, true] {
        for reference in [false, true] {
            for overlap in [false, true] {
                let mut bytes = fixture(plus, 24);
                section(&mut bytes, plus, 0, [8, 4096, 8, 512]);
                section(
                    &mut bytes,
                    plus,
                    1,
                    [16, if overlap { 4096 } else { 4104 }, 16, 520],
                );
                record(&mut bytes, 512, 1, 1, u16::from(reference));
                let start = RelativeVirtualAddress::new(4096);
                let (index, length) = if overlap {
                    (0, 8)
                } else {
                    (u16::from(!reference), 16)
                };
                let cause = if overlap {
                    PeRvaError::AmbiguousRange { start, length }
                } else {
                    PeRvaError::CrossesRegionBoundary { start, length }
                };
                let expected = if reference && !overlap {
                    PeBoundImportError::ForwarderRange {
                        descriptor_index: index,
                        count: 1,
                        start,
                        length,
                        cause,
                    }
                } else {
                    PeBoundImportError::DescriptorRange {
                        descriptor_index: index,
                        start,
                        length,
                        cause,
                    }
                };
                assert_eq!(parse_pe_bound_import_descriptors(&bytes), Err(expected));
            }
        }
    }
}

#[test]
fn conservative_section_tails_preserve_the_underlying_cause() {
    for plus in [false, true] {
        let start = RelativeVirtualAddress::new(4096);
        for (vs, raw, cause) in [
            (
                0,
                24,
                PeRvaError::ZeroVirtualSizeUnsupported {
                    start,
                    length: 8,
                    section_index: 0,
                },
            ),
            (
                4,
                24,
                PeRvaError::RawPaddingUnsupported {
                    start,
                    length: 8,
                    section_index: 0,
                },
            ),
            (
                24,
                4,
                PeRvaError::NotFileBacked {
                    start,
                    length: 8,
                    section_index: 0,
                },
            ),
        ] {
            let mut bytes = fixture(plus, 24);
            section(&mut bytes, plus, 0, [vs, 4096, raw, 512]);
            assert_eq!(
                parse_pe_bound_import_descriptors(&bytes),
                Err(PeBoundImportError::DescriptorRange {
                    descriptor_index: 0,
                    start,
                    length: 8,
                    cause
                })
            );
        }
    }
}

#[test]
fn maximum_combined_prefix_and_unrelated_directory_targets_are_supported() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 9224);
        record(&mut bytes, 512, 1, u16::MAX, 1024);
        for index in 1..128 {
            record(&mut bytes, 512 + 8192 + index * 8, 1, 0, 0);
        }
        for slot in [1, 13] {
            put32(&mut bytes, 152 + fixed(plus) + slot * 8, u32::MAX);
            put32(&mut bytes, 156 + fixed(plus) + slot * 8, u32::MAX);
        }
        let table = parse_pe_bound_import_descriptors(&bytes).unwrap().unwrap();
        assert_eq!(table.descriptors.len(), 128);
        assert_eq!(table.descriptors[0].forwarder_refs.len(), 1024);
        assert_eq!(table.terminator_rva, RelativeVirtualAddress::new(13312));
        assert_eq!(table.terminator_file_offset, FileOffset::new(9728));
    }
}
