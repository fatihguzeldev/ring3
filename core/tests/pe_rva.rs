use ring3_core::{
    FileOffset, PeFileRangeSource, PeHeaderError, PeRvaError, RelativeVirtualAddress,
    resolve_pe_file_range,
};

fn fixture(header_size: u32, sections: &[(u32, u32, u32, u32)]) -> Vec<u8> {
    let mut bytes = vec![0; 1024];
    bytes[..2].copy_from_slice(b"MZ");
    bytes[60..64].copy_from_slice(&128_u32.to_le_bytes());
    bytes[128..132].copy_from_slice(b"PE\0\0");
    bytes[134..136].copy_from_slice(&u16::try_from(sections.len()).unwrap().to_le_bytes());
    bytes[148..150].copy_from_slice(&96_u16.to_le_bytes());
    bytes[152..154].copy_from_slice(&0x10b_u16.to_le_bytes());
    bytes[212..216].copy_from_slice(&header_size.to_le_bytes());
    for (index, (va, virtual_size, raw_size, raw_pointer)) in sections.iter().enumerate() {
        let record = &mut bytes[248 + index * 40..248 + (index + 1) * 40];
        for (offset, value) in [
            (8, virtual_size),
            (12, va),
            (16, raw_size),
            (20, raw_pointer),
        ] {
            record[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
    }
    for (index, byte) in bytes[512..].iter_mut().enumerate() {
        *byte = u8::try_from(index % 251).unwrap();
    }
    bytes
}

#[test]
fn headers_and_normal_sections_resolve_to_the_original_input() {
    let bytes = fixture(512, &[(0x1000, 8, 8, 512)]);
    let headers = resolve_pe_file_range(&bytes, RelativeVirtualAddress::new(0), 2).unwrap();
    assert_eq!(headers.file_offset, FileOffset::new(0));
    assert_eq!(headers.source, PeFileRangeSource::Headers);
    assert_eq!(headers.bytes, b"MZ");
    assert!(std::ptr::eq(headers.bytes.as_ptr(), bytes.as_ptr()));
    let section = resolve_pe_file_range(&bytes, RelativeVirtualAddress::new(0x1002), 3).unwrap();
    assert_eq!(section.file_offset, FileOffset::new(514));
    assert_eq!(section.source, PeFileRangeSource::Section(0));
    assert_eq!(section.bytes, &bytes[514..517]);
    assert!(std::ptr::eq(section.bytes.as_ptr(), bytes[514..].as_ptr()));
}

#[test]
fn request_errors_precede_parsing() {
    let start = RelativeVirtualAddress::new(u32::MAX);
    assert_eq!(
        resolve_pe_file_range(&[], start, 0),
        Err(PeRvaError::EmptyRange { start })
    );
    assert_eq!(
        resolve_pe_file_range(&[], start, 2),
        Err(PeRvaError::RvaRangeOverflow { start, length: 2 })
    );
    assert_eq!(
        resolve_pe_file_range(&[], start, 1),
        Err(PeRvaError::Parse(PeHeaderError::OutOfBounds {
            offset: FileOffset::new(0),
            needed: 2,
            available: 0
        }))
    );
}

#[test]
fn ambiguity_is_requested_byte_overlap_not_adjacent_region_intersection() {
    let start = RelativeVirtualAddress::new(0x1003);
    let adjacent = fixture(512, &[(0x1000, 4, 4, 512), (0x1004, 4, 4, 516)]);
    assert_eq!(
        resolve_pe_file_range(&adjacent, start, 2),
        Err(PeRvaError::CrossesRegionBoundary { start, length: 2 })
    );
    let overlapping = fixture(512, &[(0x1000, 5, 5, 512), (0x1004, 4, 4, 516)]);
    assert_eq!(
        resolve_pe_file_range(&overlapping, start, 2),
        Err(PeRvaError::AmbiguousRange { start, length: 2 })
    );
}

#[test]
fn distinct_tail_classes_are_explicit() {
    let start = RelativeVirtualAddress::new(0x1003);
    for (virtual_size, raw_size, expected) in [
        (
            4,
            8,
            PeRvaError::RawPaddingUnsupported {
                start,
                length: 2,
                section_index: 0,
            },
        ),
        (
            8,
            4,
            PeRvaError::NotFileBacked {
                start,
                length: 2,
                section_index: 0,
            },
        ),
        (
            0,
            8,
            PeRvaError::ZeroVirtualSizeUnsupported {
                start,
                length: 2,
                section_index: 0,
            },
        ),
    ] {
        let bytes = fixture(512, &[(0x1000, virtual_size, raw_size, 512)]);
        assert_eq!(resolve_pe_file_range(&bytes, start, 2), Err(expected));
    }
}

#[test]
fn header_extent_covers_the_declared_table_and_fits_the_input() {
    let start = RelativeVirtualAddress::new(0);
    for (sections, minimum) in [(vec![], 248), (vec![(0x1000, 4, 4, 512)], 288)] {
        for size_of_headers in [minimum - 1, 1025] {
            let bytes = fixture(size_of_headers, &sections);
            assert_eq!(
                resolve_pe_file_range(&bytes, start, 1),
                Err(PeRvaError::InvalidHeaderExtent {
                    size_of_headers,
                    minimum: u64::from(minimum),
                    file_size: 1024,
                })
            );
        }
        for size_of_headers in [minimum, 1024] {
            let bytes = fixture(size_of_headers, &sections);
            assert_eq!(
                resolve_pe_file_range(&bytes, start, 1).unwrap().source,
                PeFileRangeSource::Headers
            );
        }
    }
    let mut bytes = fixture(304, &[(0x1000, 4, 4, 512)]);
    bytes.copy_within(248..288, 264);
    bytes[248..264].fill(0);
    bytes[148..150].copy_from_slice(&112_u16.to_le_bytes());
    assert!(resolve_pe_file_range(&bytes, start, 1).is_ok());
    bytes[212..216].copy_from_slice(&303_u32.to_le_bytes());
    assert_eq!(
        resolve_pe_file_range(&bytes, RelativeVirtualAddress::new(0x1000), 1),
        Err(PeRvaError::InvalidHeaderExtent {
            size_of_headers: 303,
            minimum: 304,
            file_size: 1024,
        })
    );
}

#[test]
fn parsing_all_sections_precedes_header_extent_and_resolution() {
    let bytes = fixture(1, &[(0x1000, 4, 4, 1022)]);
    assert_eq!(
        resolve_pe_file_range(&bytes, RelativeVirtualAddress::new(0), 1),
        Err(PeRvaError::Parse(
            PeHeaderError::SectionRawDataOutOfBounds {
                section_index: 0,
                section_offset: FileOffset::new(248),
                offset: FileOffset::new(1022),
                needed: 4,
                available: 2,
            }
        ))
    );
}

#[test]
fn exact_endpoints_gaps_and_adjacent_headers_never_stitch() {
    let bytes = fixture(512, &[(0x1000, 4, 4, 512), (0x1008, 4, 4, 516)]);
    assert_eq!(
        resolve_pe_file_range(&bytes, RelativeVirtualAddress::new(511), 1)
            .unwrap()
            .file_offset,
        FileOffset::new(511)
    );
    for (start, length) in [(512, 1), (0x1004, 4), (0x100c, 1), (0x2000, 3)] {
        let start = RelativeVirtualAddress::new(start);
        assert_eq!(
            resolve_pe_file_range(&bytes, start, length),
            Err(PeRvaError::UnmappedRva { start, length })
        );
    }
    for (start, length) in [(511, 2), (0xfff, 2), (0x1003, 6), (0x100b, 2)] {
        let start = RelativeVirtualAddress::new(start);
        assert_eq!(
            resolve_pe_file_range(&bytes, start, length),
            Err(PeRvaError::CrossesRegionBoundary { start, length })
        );
    }
    let adjacent = fixture(512, &[(512, 4, 4, 512)]);
    let start = RelativeVirtualAddress::new(511);
    assert_eq!(
        resolve_pe_file_range(&adjacent, start, 2),
        Err(PeRvaError::CrossesRegionBoundary { start, length: 2 })
    );
}

#[test]
fn zero_extents_have_distinct_mapping_and_guard_policies() {
    let bytes = fixture(512, &[(0x1000, 0, 0, u32::MAX), (0x1000, 4, 4, 512)]);
    assert_eq!(
        resolve_pe_file_range(&bytes, RelativeVirtualAddress::new(0x1000), 4)
            .unwrap()
            .source,
        PeFileRangeSource::Section(1)
    );
    let empty_in_headers = fixture(512, &[(1, 0, 0, u32::MAX)]);
    assert_eq!(
        resolve_pe_file_range(&empty_in_headers, RelativeVirtualAddress::new(0), 3)
            .unwrap()
            .source,
        PeFileRangeSource::Headers
    );
    let start = RelativeVirtualAddress::new(0x1000);
    let empty = fixture(512, &[(0x1000, 0, 0, u32::MAX)]);
    assert_eq!(
        resolve_pe_file_range(&empty, start, 1),
        Err(PeRvaError::UnmappedRva { start, length: 1 })
    );
    let virtual_only = fixture(512, &[(0x1000, 4, 0, u32::MAX)]);
    assert_eq!(
        resolve_pe_file_range(&virtual_only, start, 4),
        Err(PeRvaError::NotFileBacked {
            start,
            length: 4,
            section_index: 0,
        })
    );
}

#[test]
fn last_rva_byte_is_valid_and_raw_guards_clip_without_wrapping() {
    let start = RelativeVirtualAddress::new(u32::MAX);
    let bytes = fixture(512, &[(u32::MAX, 1, 1, 512)]);
    let result = resolve_pe_file_range(&bytes, start, 1).unwrap();
    assert_eq!(result.file_offset, FileOffset::new(512));
    assert_eq!(result.bytes, &bytes[512..513]);
    assert_eq!(
        resolve_pe_file_range(&bytes, start, 2),
        Err(PeRvaError::RvaRangeOverflow { start, length: 2 })
    );
    let padding = fixture(512, &[(u32::MAX - 1, 1, 8, 512)]);
    assert_eq!(
        resolve_pe_file_range(&padding, start, 1),
        Err(PeRvaError::RawPaddingUnsupported {
            start,
            length: 1,
            section_index: 0,
        })
    );
    let zero_virtual = fixture(512, &[(u32::MAX, 0, 8, 512)]);
    assert_eq!(
        resolve_pe_file_range(&zero_virtual, start, 1),
        Err(PeRvaError::ZeroVirtualSizeUnsupported {
            start,
            length: 1,
            section_index: 0,
        })
    );
    assert_eq!(
        resolve_pe_file_range(&padding, RelativeVirtualAddress::new(0), 2)
            .unwrap()
            .bytes,
        b"MZ"
    );
}

#[test]
fn all_competitor_extents_cause_ambiguity_in_either_file_order() {
    let start = RelativeVirtualAddress::new(0x1004);
    for competitor in [
        (0x1004, 4, 4, 516),
        (0x1000, 8, 8, 512),
        (0x1000, 2, 8, 512),
        (0x1000, 8, 2, 512),
        (0x1000, 0, 8, 512),
        (0x1000, 8, 0, u32::MAX),
    ] {
        let normal = (0x1000, 8, 8, 512);
        for sections in [[normal, competitor], [competitor, normal]] {
            let bytes = fixture(512, &sections);
            assert_eq!(
                resolve_pe_file_range(&bytes, start, 1),
                Err(PeRvaError::AmbiguousRange { start, length: 1 })
            );
        }
    }
    let headers = fixture(512, &[(510, 4, 4, 512)]);
    for (start, length) in [(510, 1), (509, 6)] {
        let start = RelativeVirtualAddress::new(start);
        assert_eq!(
            resolve_pe_file_range(&headers, start, length),
            Err(PeRvaError::AmbiguousRange { start, length })
        );
    }
}

#[test]
fn overlap_outside_the_request_does_not_reject_a_unique_region() {
    let first = (0x1000, 8, 8, 512);
    let second = (0x1004, 8, 8, 520);
    for (sections, index) in [([first, second], 0), ([second, first], 1)] {
        let bytes = fixture(512, &sections);
        let result = resolve_pe_file_range(&bytes, RelativeVirtualAddress::new(0x1000), 4).unwrap();
        assert_eq!(result.file_offset, FileOffset::new(512));
        assert_eq!(result.source, PeFileRangeSource::Section(index));
        assert_eq!(result.bytes, &bytes[512..516]);
        let start = RelativeVirtualAddress::new(0xfff);
        assert_eq!(
            resolve_pe_file_range(&bytes, start, 20),
            Err(PeRvaError::AmbiguousRange { start, length: 20 })
        );
    }
}

fn check_generated_fixture(variable: &str) {
    let path = std::env::var(variable).expect("an explicit generated fixture path is required");
    let bytes = std::fs::read(path).unwrap();
    let headers = resolve_pe_file_range(&bytes, RelativeVirtualAddress::new(0), 2).unwrap();
    assert_eq!(headers.file_offset, FileOffset::new(0));
    assert_eq!(headers.source, PeFileRangeSource::Headers);
    assert_eq!(headers.bytes, b"MZ");
    let start = RelativeVirtualAddress::new(0x1000);
    let entry = resolve_pe_file_range(&bytes, start, 11).unwrap();
    assert_eq!(entry.file_offset, FileOffset::new(512));
    assert_eq!(entry.source, PeFileRangeSource::Section(0));
    assert_eq!(
        entry.bytes,
        &[0xb8, 7, 0, 0, 0, 0x83, 0xc0, 35, 0xcc, 0x0f, 0x0b]
    );
    assert!(std::ptr::eq(entry.bytes.as_ptr(), bytes[512..].as_ptr()));
    assert_eq!(
        resolve_pe_file_range(&bytes, start, 12),
        Err(PeRvaError::RawPaddingUnsupported {
            start,
            length: 12,
            section_index: 0,
        })
    );
}

#[test]
#[ignore = "requires an explicitly supplied generated PE32 fixture"]
fn generated_pe32_entry_range_matches_recorded_file_bytes() {
    check_generated_fixture("RING3_PE32_FIXTURE");
}

#[test]
#[ignore = "requires an explicitly supplied generated PE32+ fixture"]
fn generated_pe32plus_entry_range_matches_recorded_file_bytes() {
    check_generated_fixture("RING3_PE32PLUS_FIXTURE");
}
