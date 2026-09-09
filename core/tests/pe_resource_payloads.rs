use ring3_core::{
    FileOffset, PeFileRange, PeFileRangeSource, PeResourceDataEntryError, PeResourceDirectoryError,
    PeResourcePayload, PeResourcePayloadError as Error, PeRvaError, RelativeVirtualAddress,
    parse_pe_resource_data_entries, parse_pe_resource_payloads, resolve_pe_file_range,
};

fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn fixed(plus: bool) -> usize {
    if plus { 112 } else { 96 }
}
fn directory(bytes: &mut [u8], plus: bool, rva: u32, size: u32) {
    put32(bytes, 152 + fixed(plus) + 16, rva);
    put32(bytes, 152 + fixed(plus) + 20, size);
}
fn section(bytes: &mut [u8], plus: bool, rva: u32, virtual_size: u32, raw_size: u32) {
    for (delta, value) in [(8, virtual_size), (12, rva), (16, raw_size), (20, 512)] {
        put32(bytes, 152 + fixed(plus) + 24 + delta, value);
    }
}
fn table(bytes: &mut [u8], offset: u32, targets: &[u32]) {
    let at = 512 + offset as usize;
    bytes[at..at + 16].fill(0);
    bytes[at + 14..at + 16].copy_from_slice(&u16::try_from(targets.len()).unwrap().to_le_bytes());
    for (index, &target) in targets.iter().enumerate() {
        put32(bytes, at + 16 + 8 * index, u32::MAX);
        put32(bytes, at + 20 + 8 * index, target);
    }
}
fn fixture(plus: bool, targets: &[u32], records: &[(u32, [u32; 4])], size: u32) -> Vec<u8> {
    let mut bytes = vec![0; 512 + size as usize];
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 60, 128);
    bytes[128..132].copy_from_slice(b"PE\0\0");
    bytes[134..136].copy_from_slice(&1_u16.to_le_bytes());
    bytes[148..150].copy_from_slice(&u16::try_from(fixed(plus) + 24).unwrap().to_le_bytes());
    bytes[152..154].copy_from_slice(&if plus { 0x20b_u16 } else { 0x10b }.to_le_bytes());
    put32(&mut bytes, 212, 512);
    put32(&mut bytes, 152 + fixed(plus) - 4, 3);
    directory(&mut bytes, plus, 4096, size);
    section(&mut bytes, plus, 4096, size, size);
    table(&mut bytes, 0, targets);
    for &(offset, fields) in records {
        for (i, value) in fields.into_iter().enumerate() {
            put32(&mut bytes, 512 + offset as usize + 4 * i, value);
        }
    }
    bytes
}

fn extra_section(
    bytes: &mut Vec<u8>,
    plus: bool,
    index: u16,
    rva: u32,
    virtual_size: u32,
    raw_size: u32,
) {
    let file = u32::try_from(bytes.len()).unwrap();
    bytes.resize(bytes.len() + raw_size as usize, 0x5a);
    bytes[134..136].copy_from_slice(&(index + 1).to_le_bytes());
    let at = 152 + fixed(plus) + 24 + usize::from(index) * 40;
    for (delta, value) in [(8, virtual_size), (12, rva), (16, raw_size), (20, file)] {
        put32(bytes, at + delta, value);
    }
}
fn payload(rva: u32, size: u32) -> [u32; 4] {
    [rva, size, u32::MAX, u32::MAX]
}
fn expected_range(
    bytes: &[u8],
    file: usize,
    length: usize,
    source: PeFileRangeSource,
) -> PeFileRange<'_> {
    PeFileRange {
        file_offset: FileOffset::new(file as u64),
        bytes: &bytes[file..file + length],
        source,
    }
}

#[test]
fn views_borrow_exact_bytes_and_preserve_record_order_indices_and_metadata() {
    for plus in [false, true] {
        let mut bytes = fixture(
            plus,
            &[64, 48, 64, 80],
            &[
                (48, payload(4354, 2)),
                (64, payload(4352, 4)),
                (80, payload(u32::MAX, 0)),
            ],
            512,
        );
        bytes[768..772].copy_from_slice(&[0x12, 0x34, 0x56, 0x78]);
        let before = bytes.clone();
        let parsed = parse_pe_resource_payloads(&bytes).unwrap().unwrap();
        assert_eq!(
            Some(parsed.data_entry_table.clone()),
            parse_pe_resource_data_entries(&bytes).unwrap()
        );
        assert_eq!(
            parsed.payloads,
            [
                PeResourcePayload {
                    data_entry_index: 0,
                    range: Some(expected_range(
                        &bytes,
                        768,
                        4,
                        PeFileRangeSource::Section(0)
                    ))
                },
                PeResourcePayload {
                    data_entry_index: 1,
                    range: Some(expected_range(
                        &bytes,
                        770,
                        2,
                        PeFileRangeSource::Section(0)
                    ))
                },
                PeResourcePayload {
                    data_entry_index: 2,
                    range: None
                },
            ]
        );
        assert_eq!(
            parsed.payloads[0].range.unwrap().bytes.as_ptr(),
            bytes[768..].as_ptr()
        );
        assert_eq!(
            parsed.payloads[1].range.unwrap().bytes.as_ptr(),
            bytes[770..].as_ptr()
        );
        assert_eq!(
            parsed
                .data_entry_table
                .references
                .iter()
                .map(|r| r.data_entry_index)
                .collect::<Vec<_>>(),
            [0, 1, 0, 2]
        );
        assert_eq!(bytes, before);
    }
}

#[test]
fn absent_and_present_tables_without_data_keep_distinct_results() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, &[], &[], 16);
        let parsed = parse_pe_resource_payloads(&bytes).unwrap().unwrap();
        assert!(parsed.payloads.is_empty());
        assert!(parsed.data_entry_table.data_entries.is_empty());
        directory(&mut bytes, plus, 0, 0);
        assert_eq!(parse_pe_resource_payloads(&bytes), Ok(None));
        let mut bytes = fixture(plus, &[0x8000_0018], &[], 40);
        table(&mut bytes, 24, &[]);
        assert!(
            parse_pe_resource_payloads(&bytes)
                .unwrap()
                .unwrap()
                .payloads
                .is_empty()
        );
    }
}

#[test]
fn empty_payloads_keep_arbitrary_raw_rvas_without_physical_coordinates() {
    for plus in [false, true] {
        let bytes = fixture(
            plus,
            &[48, 64, 80],
            &[
                (48, payload(0, 0)),
                (64, payload(16384, 0)),
                (80, payload(u32::MAX, 0)),
            ],
            96,
        );
        let parsed = parse_pe_resource_payloads(&bytes).unwrap().unwrap();
        for (index, rva) in [0, 16384, u32::MAX].into_iter().enumerate() {
            assert_eq!(
                parsed.payloads[index],
                PeResourcePayload {
                    data_entry_index: u16::try_from(index).unwrap(),
                    range: None
                }
            );
            assert_eq!(
                parsed.data_entry_table.data_entries[index]
                    .payload_rva
                    .get(),
                rva
            );
            assert_eq!(
                resolve_pe_file_range(&bytes, RelativeVirtualAddress::new(rva), 0),
                Err(PeRvaError::EmptyRange {
                    start: RelativeVirtualAddress::new(rva)
                })
            );
        }
    }
}

#[test]
fn payloads_use_image_rvas_for_headers_outside_directory_and_other_sections() {
    for plus in [false, true] {
        for (rva, size, file, source) in [
            (0, 2, 0, PeFileRangeSource::Headers),
            (4352, 4, 768, PeFileRangeSource::Section(0)),
            (8192, 4, 1024, PeFileRangeSource::Section(1)),
        ] {
            let mut bytes = fixture(plus, &[32], &[(32, payload(rva, size))], 512);
            directory(&mut bytes, plus, 4096, 128);
            extra_section(&mut bytes, plus, 1, 8192, 16, 16);
            let parsed = parse_pe_resource_payloads(&bytes).unwrap().unwrap();
            assert_eq!(
                parsed.payloads[0].range,
                Some(expected_range(&bytes, file, size as usize, source))
            );
        }
    }
}

#[test]
fn whole_graph_and_all_data_records_are_checked_before_any_payload() {
    for plus in [false, true] {
        let bytes = fixture(plus, &[48, 0x8000_0000], &[(48, payload(u32::MAX, 4))], 128);
        assert_eq!(
            parse_pe_resource_payloads(&bytes),
            Err(Error::Data(PeResourceDataEntryError::Graph(
                PeResourceDirectoryError::Cycle {
                    remaining_directories: 1
                }
            )))
        );
        let bytes = fixture(plus, &[48, 0x7fff_ffff], &[(48, payload(u32::MAX, 4))], 128);
        assert_eq!(
            parse_pe_resource_payloads(&bytes),
            Err(Error::Data(
                PeResourceDataEntryError::DataEntryOutsideResource {
                    directory_index: 0,
                    entry_index: 1,
                    offset: 0x7fff_ffff,
                    length: 16,
                    directory_size: 128
                }
            ))
        );
    }
}

#[test]
fn first_invalid_distinct_payload_retains_record_index_and_complete_cause() {
    for plus in [false, true] {
        let bytes = fixture(
            plus,
            &[48, 48, 64],
            &[(48, payload(4352, 4)), (64, payload(u32::MAX, 2))],
            512,
        );
        assert!(parse_pe_resource_data_entries(&bytes).is_ok());
        let start = RelativeVirtualAddress::new(u32::MAX);
        assert_eq!(
            parse_pe_resource_payloads(&bytes),
            Err(Error::PayloadRange {
                data_entry_index: 1,
                start,
                length: 2,
                cause: PeRvaError::RvaRangeOverflow { start, length: 2 }
            })
        );
    }
}

#[test]
fn unmapped_and_crossing_requests_are_not_stitched() {
    for plus in [false, true] {
        for (rva, adjacent) in [(16384, false), (4606, false), (4606, true)] {
            let mut bytes = fixture(plus, &[32], &[(32, payload(rva, 4))], 512);
            if adjacent {
                extra_section(&mut bytes, plus, 1, 4608, 16, 16);
            }
            let start = RelativeVirtualAddress::new(rva);
            let cause = if rva == 16384 {
                PeRvaError::UnmappedRva { start, length: 4 }
            } else {
                PeRvaError::CrossesRegionBoundary { start, length: 4 }
            };
            assert_eq!(
                parse_pe_resource_payloads(&bytes),
                Err(Error::PayloadRange {
                    data_entry_index: 0,
                    start,
                    length: 4,
                    cause
                })
            );
        }
    }
}

#[test]
fn unsupported_section_tails_retain_their_specific_causes() {
    for plus in [false, true] {
        for (virtual_size, raw_size, rva) in [(0, 8, 8192), (4, 8, 8196), (8, 4, 8196)] {
            let mut bytes = fixture(plus, &[32], &[(32, payload(rva, 2))], 512);
            extra_section(&mut bytes, plus, 1, 8192, virtual_size, raw_size);
            let start = RelativeVirtualAddress::new(rva);
            let cause = match virtual_size {
                0 => PeRvaError::ZeroVirtualSizeUnsupported {
                    start,
                    length: 2,
                    section_index: 1,
                },
                4 => PeRvaError::RawPaddingUnsupported {
                    start,
                    length: 2,
                    section_index: 1,
                },
                _ => PeRvaError::NotFileBacked {
                    start,
                    length: 2,
                    section_index: 1,
                },
            };
            assert_eq!(
                parse_pe_resource_payloads(&bytes),
                Err(Error::PayloadRange {
                    data_entry_index: 0,
                    start,
                    length: 2,
                    cause
                })
            );
        }
    }
}

#[test]
fn overlaps_are_ambiguous_only_where_the_payload_requests_bytes() {
    for plus in [false, true] {
        for overlap in [8194, 8196] {
            let mut bytes = fixture(plus, &[32], &[(32, payload(8192, 4))], 512);
            extra_section(&mut bytes, plus, 1, 8192, 16, 16);
            extra_section(&mut bytes, plus, 2, overlap, 4, 4);
            let parsed = parse_pe_resource_payloads(&bytes);
            if overlap == 8194 {
                let start = RelativeVirtualAddress::new(8192);
                assert_eq!(
                    parsed,
                    Err(Error::PayloadRange {
                        data_entry_index: 0,
                        start,
                        length: 4,
                        cause: PeRvaError::AmbiguousRange { start, length: 4 }
                    })
                );
            } else {
                assert_eq!(
                    parsed.unwrap().unwrap().payloads[0].range.unwrap().bytes,
                    &[0x5a; 4]
                );
            }
        }
    }
}

#[test]
fn the_last_payload_byte_can_end_exactly_at_the_rva_domain_boundary() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, &[32], &[(32, payload(u32::MAX - 3, 4))], 512);
        let origin = u32::MAX - 511;
        directory(&mut bytes, plus, origin, 128);
        section(&mut bytes, plus, origin, 512, 512);
        let parsed = parse_pe_resource_payloads(&bytes).unwrap().unwrap();
        assert_eq!(
            parsed.payloads[0].range,
            Some(expected_range(
                &bytes,
                1020,
                4,
                PeFileRangeSource::Section(0)
            ))
        );
    }
}

#[test]
fn shared_directory_paths_and_records_do_not_multiply_payload_views() {
    for plus in [false, true] {
        for shared in [false, true] {
            let mut bytes = fixture(
                plus,
                &[0x8000_0020, if shared { 0x8000_0020 } else { 0x8000_0038 }],
                &[(80, payload(4352, 4))],
                512,
            );
            table(&mut bytes, 32, if shared { &[80, 80] } else { &[80] });
            if !shared {
                table(&mut bytes, 56, &[80]);
            }
            let parsed = parse_pe_resource_payloads(&bytes).unwrap().unwrap();
            assert_eq!(parsed.payloads.len(), 1);
            assert_eq!(parsed.data_entry_table.references.len(), 2);
            assert_eq!(
                parsed.payloads[0].range.unwrap().bytes.as_ptr(),
                bytes[768..].as_ptr()
            );
        }
    }
}

#[test]
fn graph_entry_limits_bound_many_distinct_views_of_one_payload() {
    for plus in [false, true] {
        let data_start = 16 * 2064;
        let payload_start = data_start + 4081 * 16;
        let mut bytes = fixture(plus, &[], &[], payload_start + 4);
        let mut next_record = 0;
        for node in 0..16 {
            let mut targets = Vec::new();
            for entry in 0..256 {
                if node == 0 && entry < 15 {
                    targets.push(0x8000_0000 | ((entry + 1) * 2064));
                } else {
                    let offset = data_start + 16 * next_record;
                    targets.push(offset);
                    put32(&mut bytes, 512 + offset as usize, 4096 + payload_start);
                    put32(&mut bytes, 516 + offset as usize, 4);
                    next_record += 1;
                }
            }
            table(&mut bytes, node * 2064, &targets);
        }
        assert_eq!(next_record, 4081);
        let parsed = parse_pe_resource_payloads(&bytes).unwrap().unwrap();
        assert_eq!(parsed.payloads.len(), 4081);
        assert_eq!(parsed.data_entry_table.references.len(), 4081);
        let pointer = bytes[512 + payload_start as usize..].as_ptr();
        for (index, view) in parsed.payloads.iter().enumerate() {
            assert_eq!(usize::from(view.data_entry_index), index);
            assert_eq!(view.range.unwrap().bytes.as_ptr(), pointer);
            assert_eq!(view.range.unwrap().bytes.len(), 4);
        }
    }
}
