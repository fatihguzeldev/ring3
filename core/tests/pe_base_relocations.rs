use ring3_core::{
    FileOffset, PeBaseRelocationBlock, PeBaseRelocationError, PeDirectoryAddress, PeHeaderError,
    PeKind, PeRvaError, RelativeVirtualAddress, parse_pe_base_relocation_blocks, parse_pe_headers,
};

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn fixed(plus: bool) -> usize {
    if plus { 112 } else { 96 }
}

fn directory(bytes: &mut [u8], plus: bool, rva: u32, size: u32) {
    put32(bytes, 152 + fixed(plus) + 40, rva);
    put32(bytes, 152 + fixed(plus) + 44, size);
}

fn section(bytes: &mut [u8], plus: bool, index: usize, fields: [u32; 4]) {
    let table = 152 + fixed(plus) + 48 + index * 40;
    for (offset, value) in [8, 12, 16, 20].into_iter().zip(fields) {
        put32(bytes, table + offset, value);
    }
}

fn fixture(plus: bool, raw_size: u32) -> Vec<u8> {
    let mut bytes = vec![0; 512 + raw_size as usize + 32];
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 60, 128);
    bytes[128..132].copy_from_slice(b"PE\0\0");
    bytes[134..136].copy_from_slice(&2_u16.to_le_bytes());
    let optional_size = u16::try_from(fixed(plus) + 48).unwrap();
    bytes[148..150].copy_from_slice(&optional_size.to_le_bytes());
    bytes[152..154].copy_from_slice(&if plus { 0x20b_u16 } else { 0x10b }.to_le_bytes());
    put32(&mut bytes, 212, 512);
    put32(&mut bytes, 152 + fixed(plus) - 4, 6);
    directory(&mut bytes, plus, 0x1000, 12);
    section(&mut bytes, plus, 0, [raw_size, 0x1000, raw_size, 512]);
    section(
        &mut bytes,
        plus,
        1,
        [32, 0x3000 + raw_size, 32, 512 + raw_size],
    );
    put32(&mut bytes, 512, 0x2000);
    put32(&mut bytes, 516, 12);
    bytes[520..524].copy_from_slice(&[0x34, 0x31, 0, 0]);
    bytes
}

fn block(page: u32, size: u32) -> PeBaseRelocationBlock<'static> {
    PeBaseRelocationBlock {
        block_rva: RelativeVirtualAddress::new(0x1000),
        block_file_offset: FileOffset::new(512),
        page_rva: RelativeVirtualAddress::new(page),
        block_size: size,
        raw_entries: &[0x34, 0x31, 0, 0],
    }
}

#[test]
fn both_widths_preserve_borrowed_raw_blocks_without_interpreting_slots() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 64);
        let words = [0x4001_u16, 0xffff, 0, 0xafff, 0x6123, 0x6123];
        directory(&mut bytes, plus, 0x1000, 28);
        put32(&mut bytes, 512, u32::MAX);
        put32(&mut bytes, 516, 20);
        for (slot, value) in bytes[520..532].chunks_exact_mut(2).zip(words) {
            slot.copy_from_slice(&value.to_le_bytes());
        }
        put32(&mut bytes, 532, u32::MAX);
        put32(&mut bytes, 536, 8);
        let before = bytes.clone();
        let blocks = parse_pe_base_relocation_blocks(&bytes).unwrap();
        assert_eq!(
            blocks,
            vec![
                PeBaseRelocationBlock {
                    raw_entries: &bytes[520..532],
                    ..block(u32::MAX, 20)
                },
                PeBaseRelocationBlock {
                    block_rva: RelativeVirtualAddress::new(0x1014),
                    block_file_offset: FileOffset::new(532),
                    page_rva: RelativeVirtualAddress::new(u32::MAX),
                    block_size: 8,
                    raw_entries: &[],
                },
            ]
        );
        assert_eq!(blocks[0].raw_entries.as_ptr(), bytes[520..].as_ptr());
        assert_eq!(bytes, before);
    }
}

#[test]
fn absent_slots_are_empty_but_present_empty_blocks_are_retained() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 32);
        directory(&mut bytes, plus, 0x1000, 8);
        put32(&mut bytes, 516, 8);
        assert_eq!(
            parse_pe_base_relocation_blocks(&bytes),
            Ok(vec![PeBaseRelocationBlock {
                raw_entries: &[],
                ..block(0x2000, 8)
            },])
        );
        directory(&mut bytes, plus, 0, 0);
        assert_eq!(parse_pe_base_relocation_blocks(&bytes), Ok(vec![]));
        directory(&mut bytes, plus, u32::MAX, u32::MAX);
        put32(&mut bytes, 152 + fixed(plus) - 4, 5);
        assert_eq!(parse_pe_base_relocation_blocks(&bytes), Ok(vec![]));
    }
}

#[test]
fn base_validation_precedes_absence_and_table_errors() {
    for plus in [false, true] {
        for (rva, size) in [(0, 0), (0, 7), (u32::MAX, 2), (0x1000, 12)] {
            let mut bytes = fixture(plus, 32);
            directory(&mut bytes, plus, rva, size);
            section(&mut bytes, plus, 1, [32, 0x3000, 32, 1000]);
            assert!(matches!(
                parse_pe_base_relocation_blocks(&bytes),
                Err(PeBaseRelocationError::Base(PeRvaError::Parse(
                    PeHeaderError::SectionRawDataOutOfBounds {
                        section_index: 1,
                        ..
                    }
                )))
            ));
            bytes[0] = 0;
            assert_eq!(
                parse_pe_base_relocation_blocks(&bytes),
                Err(PeBaseRelocationError::Base(PeRvaError::Parse(
                    PeHeaderError::InvalidDosSignature {
                        offset: FileOffset::new(0)
                    }
                )))
            );
        }
    }
}

#[test]
fn directory_consistency_and_wide_end_precede_traversal() {
    for plus in [false, true] {
        for (rva, size) in [(0, 1), (0x1000, 0)] {
            let mut bytes = fixture(plus, 32);
            directory(&mut bytes, plus, rva, size);
            assert_eq!(
                parse_pe_base_relocation_blocks(&bytes),
                Err(PeBaseRelocationError::InconsistentDirectory {
                    rva: RelativeVirtualAddress::new(rva),
                    size,
                })
            );
        }
        let mut bytes = fixture(plus, 32);
        directory(&mut bytes, plus, u32::MAX, 2);
        assert_eq!(
            parse_pe_base_relocation_blocks(&bytes),
            Err(PeBaseRelocationError::DirectoryRangeOverflow {
                rva: RelativeVirtualAddress::new(u32::MAX),
                size: 2,
            })
        );
        directory(&mut bytes, plus, 0xffff_fff0, 16);
        section(&mut bytes, plus, 0, [16, 0xffff_fff0, 16, 512]);
        put32(&mut bytes, 516, 16);
        let blocks = parse_pe_base_relocation_blocks(&bytes).unwrap();
        assert_eq!(blocks[0].block_rva.get(), 0xffff_fff0);
        assert_eq!(blocks[0].raw_entries, &bytes[520..528]);
        directory(&mut bytes, plus, 0xffff_fff0, 17);
        assert!(matches!(
            parse_pe_base_relocation_blocks(&bytes),
            Err(PeBaseRelocationError::DirectoryRangeOverflow { .. })
        ));
    }
}

#[test]
fn exact_directory_end_has_no_zero_sentinel_or_unadvertised_tail() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 32);
        bytes[524..].fill(0xff);
        assert_eq!(
            parse_pe_base_relocation_blocks(&bytes),
            Ok(vec![block(0x2000, 12)])
        );
        directory(&mut bytes, plus, 0x1000, 20);
        bytes[524..532].fill(0);
        assert_eq!(
            parse_pe_base_relocation_blocks(&bytes),
            Err(PeBaseRelocationError::InvalidBlockSize {
                block_index: 1,
                block_size: 0
            })
        );
    }
}

#[test]
fn final_two_byte_slot_is_allowed_but_a_following_unaligned_block_is_not() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 32);
        directory(&mut bytes, plus, 0x1000, 10);
        put32(&mut bytes, 516, 10);
        let blocks = parse_pe_base_relocation_blocks(&bytes).unwrap();
        assert_eq!(blocks[0].raw_entries, &[0x34, 0x31]);
        directory(&mut bytes, plus, 0x1000, 11);
        assert_eq!(
            parse_pe_base_relocation_blocks(&bytes),
            Err(PeBaseRelocationError::UnalignedBlock {
                block_index: 1,
                block_rva: RelativeVirtualAddress::new(0x100a),
            })
        );
        directory(&mut bytes, plus, 0x1002, 1);
        assert_eq!(
            parse_pe_base_relocation_blocks(&bytes),
            Err(PeBaseRelocationError::UnalignedBlock {
                block_index: 0,
                block_rva: RelativeVirtualAddress::new(0x1002),
            })
        );
    }
}

#[test]
fn short_headers_and_invalid_sizes_report_the_first_structural_error() {
    for plus in [false, true] {
        for remaining in 1..8 {
            let mut bytes = fixture(plus, 32);
            directory(&mut bytes, plus, 0x9000, remaining);
            assert_eq!(
                parse_pe_base_relocation_blocks(&bytes),
                Err(PeBaseRelocationError::TruncatedBlockHeader {
                    block_index: 0,
                    remaining
                })
            );
        }
        for block_size in [0, 1, 6, 7, 9, 11, u32::MAX] {
            let mut bytes = fixture(plus, 32);
            put32(&mut bytes, 516, block_size);
            assert_eq!(
                parse_pe_base_relocation_blocks(&bytes),
                Err(PeBaseRelocationError::InvalidBlockSize {
                    block_index: 0,
                    block_size
                })
            );
        }
        let mut bytes = fixture(plus, 32);
        put32(&mut bytes, 516, u32::MAX - 1);
        assert_eq!(
            parse_pe_base_relocation_blocks(&bytes),
            Err(PeBaseRelocationError::BlockExceedsDirectory {
                block_index: 0,
                block_size: u32::MAX - 1,
                remaining: 12,
            })
        );
        directory(&mut bytes, plus, 0x1000, 13);
        put32(&mut bytes, 516, 12);
        assert_eq!(
            parse_pe_base_relocation_blocks(&bytes),
            Err(PeBaseRelocationError::TruncatedBlockHeader {
                block_index: 1,
                remaining: 1
            })
        );
    }
}

#[test]
fn consumed_prefixes_require_one_unambiguous_file_backed_region() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 32);
        directory(&mut bytes, plus, 0x9000, 12);
        assert_eq!(
            parse_pe_base_relocation_blocks(&bytes),
            Err(PeBaseRelocationError::BlockRange {
                block_index: 0,
                start: RelativeVirtualAddress::new(0x9000),
                length: 8,
                cause: PeRvaError::UnmappedRva {
                    start: RelativeVirtualAddress::new(0x9000),
                    length: 8,
                },
            })
        );
        for (fields, cause_kind) in [
            ([32, 0x1000, 8, 512], "backing"),
            ([8, 0x1000, 32, 512], "padding"),
            ([0, 0x1000, 32, 512], "zero"),
        ] {
            directory(&mut bytes, plus, 0x1000, 12);
            section(&mut bytes, plus, 0, fields);
            let expected = match cause_kind {
                "backing" => PeRvaError::NotFileBacked {
                    start: RelativeVirtualAddress::new(0x1000),
                    length: 12,
                    section_index: 0,
                },
                "padding" => PeRvaError::RawPaddingUnsupported {
                    start: RelativeVirtualAddress::new(0x1000),
                    length: 12,
                    section_index: 0,
                },
                _ => PeRvaError::ZeroVirtualSizeUnsupported {
                    start: RelativeVirtualAddress::new(0x1000),
                    length: 8,
                    section_index: 0,
                },
            };
            assert_eq!(
                parse_pe_base_relocation_blocks(&bytes),
                Err(PeBaseRelocationError::BlockRange {
                    block_index: 0,
                    start: RelativeVirtualAddress::new(0x1000),
                    length: if cause_kind == "zero" { 8 } else { 12 },
                    cause: expected,
                })
            );
        }
        section(&mut bytes, plus, 0, [8, 0x1000, 8, 512]);
        section(&mut bytes, plus, 1, [32, 0x1008, 32, 544]);
        assert!(matches!(
            parse_pe_base_relocation_blocks(&bytes),
            Err(PeBaseRelocationError::BlockRange {
                length: 12,
                cause: PeRvaError::CrossesRegionBoundary { .. },
                ..
            })
        ));
        section(&mut bytes, plus, 0, [32, 0x1000, 32, 512]);
        assert!(matches!(
            parse_pe_base_relocation_blocks(&bytes),
            Err(PeBaseRelocationError::BlockRange {
                length: 12,
                cause: PeRvaError::AmbiguousRange { .. },
                ..
            })
        ));
    }
}

#[test]
fn later_blocks_cannot_switch_to_an_adjacent_backing_region() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 32);
        directory(&mut bytes, plus, 0x1000, 16);
        section(&mut bytes, plus, 0, [8, 0x1000, 8, 512]);
        section(&mut bytes, plus, 1, [32, 0x1008, 32, 544]);
        put32(&mut bytes, 516, 8);
        put32(&mut bytes, 548, 8);
        assert!(matches!(
            parse_pe_base_relocation_blocks(&bytes),
            Err(PeBaseRelocationError::BlockRange {
                block_index: 1,
                length: 16,
                cause: PeRvaError::CrossesRegionBoundary { .. },
                ..
            })
        ));
    }
}

#[test]
fn header_backing_and_unknown_machine_flags_do_not_imply_loadability() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 32);
        bytes[132..134].copy_from_slice(&0x1234_u16.to_le_bytes());
        bytes[150..152].copy_from_slice(&u16::MAX.to_le_bytes());
        directory(&mut bytes, plus, 400, 12);
        bytes.copy_within(512..524, 400);
        put32(&mut bytes, 400, 3);
        let blocks = parse_pe_base_relocation_blocks(&bytes).unwrap();
        assert_eq!(
            blocks[0],
            PeBaseRelocationBlock {
                block_rva: RelativeVirtualAddress::new(400),
                block_file_offset: FileOffset::new(400),
                page_rva: RelativeVirtualAddress::new(3),
                block_size: 12,
                raw_entries: &bytes[408..412],
            }
        );
    }
}

#[test]
fn exact_block_budget_succeeds_and_the_next_block_refuses_before_reading() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 2048);
        directory(&mut bytes, plus, 0x1000, 2048);
        for offset in (512..2560).step_by(8) {
            put32(&mut bytes, offset, 0x123);
            put32(&mut bytes, offset + 4, 8);
        }
        assert_eq!(parse_pe_base_relocation_blocks(&bytes).unwrap().len(), 256);
        directory(&mut bytes, plus, 0x1000, 2049);
        assert_eq!(
            parse_pe_base_relocation_blocks(&bytes),
            Err(PeBaseRelocationError::BlockLimitExceeded {
                block_index: 256,
                limit: 256
            })
        );
    }
}

#[test]
fn exact_total_slot_budget_succeeds_and_excess_precedes_payload_resolution() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 131_092);
        directory(&mut bytes, plus, 0x1000, 131_088);
        put32(&mut bytes, 516, 65_544);
        put32(&mut bytes, 512 + 65_544, 0x2000);
        put32(&mut bytes, 516 + 65_544, 65_544);
        let blocks = parse_pe_base_relocation_blocks(&bytes).unwrap();
        assert_eq!(
            blocks
                .iter()
                .map(|b| b.raw_entries.len() / 2)
                .sum::<usize>(),
            65_536
        );
        directory(&mut bytes, plus, 0x1000, 131_090);
        put32(&mut bytes, 516 + 65_544, 65_546);
        section(&mut bytes, plus, 0, [131_092, 0x1000, 65_552, 512]);
        assert_eq!(
            parse_pe_base_relocation_blocks(&bytes),
            Err(PeBaseRelocationError::EntrySlotLimitExceeded {
                block_index: 1,
                total_slots: 32_768,
                block_slots: 32_769,
                limit: 65_536,
            })
        );
    }
}

fn check_real_fixture(variable: &str, plus: bool) {
    let path = std::env::var_os(variable)
        .expect("an explicit generated base-relocation fixture path is required");
    let bytes = std::fs::read(path).unwrap();
    assert_eq!(bytes.len(), 6144);
    let headers = parse_pe_headers(&bytes).unwrap();
    assert_eq!(
        headers.prefix.kind,
        if plus { PeKind::Pe32Plus } else { PeKind::Pe32 }
    );
    let directory = headers.directories[5].unwrap();
    assert_eq!(
        directory.address,
        PeDirectoryAddress::Rva(RelativeVirtualAddress::new(16384))
    );
    assert_eq!(directory.size, 24);
    let raw = if plus {
        [[0x08, 0xa0, 0x10, 0xa0], [0x18, 0xa0, 0, 0]]
    } else {
        [[0x08, 0x30, 0x0c, 0x30], [0x10, 0x30, 0, 0]]
    };
    let blocks = parse_pe_base_relocation_blocks(&bytes).unwrap();
    assert_eq!(blocks.len(), 2);
    for (index, (actual, expected_raw)) in blocks.iter().zip(&raw).enumerate() {
        let index = u32::try_from(index).unwrap();
        assert_eq!(
            *actual,
            PeBaseRelocationBlock {
                block_rva: RelativeVirtualAddress::new(16384 + index * 12),
                block_file_offset: FileOffset::new(5632 + u64::from(index) * 12),
                page_rva: RelativeVirtualAddress::new(8192 + index * 4096),
                block_size: 12,
                raw_entries: expected_raw,
            }
        );
        assert_eq!(
            actual.raw_entries.as_ptr(),
            bytes[5640 + index as usize * 12..].as_ptr()
        );
    }
}

#[test]
#[ignore = "requires an explicit self-authored PE32 relocation fixture"]
fn generated_pe32_relocation_blocks_match_raw_metadata() {
    check_real_fixture("RING3_RELOCATION_PE32_FIXTURE", false);
}

#[test]
#[ignore = "requires an explicit self-authored PE32+ relocation fixture"]
fn generated_pe32plus_relocation_blocks_match_raw_metadata() {
    check_real_fixture("RING3_RELOCATION_PE32PLUS_FIXTURE", true);
}
