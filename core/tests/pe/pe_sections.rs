use ring3_core::{FileOffset, PeHeaderError, RelativeVirtualAddress, parse_pe_sections};

const OPTIONAL_OFFSET: usize = 0x98;

fn fixture(plus: bool, optional_size: u16, count: u16) -> Vec<u8> {
    let mut bytes = vec![0; OPTIONAL_OFFSET + usize::from(optional_size) + usize::from(count) * 40];
    bytes[..2].copy_from_slice(b"MZ");
    bytes[0x3c..0x40].copy_from_slice(&0x80_u32.to_le_bytes());
    bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
    bytes[0x86..0x88].copy_from_slice(&count.to_le_bytes());
    bytes[0x94..0x96].copy_from_slice(&optional_size.to_le_bytes());
    bytes[OPTIONAL_OFFSET..OPTIONAL_OFFSET + 2]
        .copy_from_slice(&(if plus { 0x20b_u16 } else { 0x10b }).to_le_bytes());
    bytes
}

#[test]
fn both_layouts_decode_all_fields_and_borrow_the_original_raw_bytes() {
    for (plus, optional_size) in [(false, 104), (true, 120)] {
        let mut bytes = fixture(plus, optional_size, 1);
        let table_offset = OPTIONAL_OFFSET + usize::from(optional_size);
        let raw_offset = bytes.len();
        let record = &mut bytes[table_offset..table_offset + 40];
        record[..8].copy_from_slice(&[0xff, 0, b'/', b'7', 1, 2, 3, 0x80]);
        for (offset, value) in [
            (8, 0x1020),
            (12, 0x3040),
            (16, 3),
            (20, u32::try_from(raw_offset).unwrap()),
            (24, u32::MAX),
            (28, u32::MAX - 1),
            (36, 0xdead_beef),
        ] {
            record[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        record[32..34].copy_from_slice(&0xabcd_u16.to_le_bytes());
        record[34..36].copy_from_slice(&0x1234_u16.to_le_bytes());
        bytes.extend_from_slice(&[7, 8, 9]);
        let table = parse_pe_sections(&bytes).unwrap();
        let section = &table.sections[0];
        assert_eq!(section.name, [0xff, 0, b'/', b'7', 1, 2, 3, 0x80]);
        assert_eq!(section.virtual_size, 0x1020);
        assert_eq!(section.virtual_address, RelativeVirtualAddress::new(0x3040));
        assert_eq!(section.size_of_raw_data, 3);
        assert_eq!(
            section.pointer_to_raw_data,
            FileOffset::new(raw_offset as u64)
        );
        assert_eq!(
            section.pointer_to_relocations,
            FileOffset::new(u64::from(u32::MAX))
        );
        assert_eq!(
            section.pointer_to_line_numbers,
            FileOffset::new(u64::from(u32::MAX) - 1)
        );
        assert_eq!(section.number_of_relocations, 0xabcd);
        assert_eq!(section.number_of_line_numbers, 0x1234);
        assert_eq!(section.characteristics, 0xdead_beef);
        assert_eq!(section.raw_data, &[7, 8, 9]);
        assert!(std::ptr::eq(
            section.raw_data.as_ptr(),
            bytes[raw_offset..].as_ptr()
        ));
    }
}

#[test]
fn zero_and_budget_limit_counts_succeed_but_larger_counts_are_scope_errors() {
    for count in [0, 96] {
        assert_eq!(
            parse_pe_sections(&fixture(false, 96, count))
                .unwrap()
                .sections
                .len(),
            usize::from(count)
        );
    }
    for count in [97, u16::MAX] {
        let mut bytes = fixture(false, 96, 0);
        bytes[0x86..0x88].copy_from_slice(&count.to_le_bytes());
        assert_eq!(
            parse_pe_sections(&bytes),
            Err(PeHeaderError::SectionLimitExceeded {
                offset: FileOffset::new(0x86),
                count,
                limit: 96,
            })
        );
    }
}

#[test]
fn last_virtual_byte_is_valid_but_an_end_beyond_u32_space_fails() {
    let mut bytes = fixture(false, 96, 1);
    let table_offset = OPTIONAL_OFFSET + 96;
    bytes[table_offset + 12..table_offset + 16].copy_from_slice(&u32::MAX.to_le_bytes());
    bytes[table_offset + 8..table_offset + 12].copy_from_slice(&1_u32.to_le_bytes());
    assert!(parse_pe_sections(&bytes).is_ok());
    bytes[table_offset + 8..table_offset + 12].copy_from_slice(&2_u32.to_le_bytes());
    assert_eq!(
        parse_pe_sections(&bytes),
        Err(PeHeaderError::VirtualRangeOverflow {
            section_index: 0,
            offset: FileOffset::new(table_offset as u64),
            virtual_address: RelativeVirtualAddress::new(u32::MAX),
            virtual_size: 2,
        })
    );
}

#[test]
fn every_required_table_and_raw_truncation_reports_its_context() {
    let mut bytes = fixture(false, 96, 2);
    let table_offset = OPTIONAL_OFFSET + 96;
    let raw_offset = bytes.len();
    bytes[table_offset + 16..table_offset + 20].copy_from_slice(&3_u32.to_le_bytes());
    bytes[table_offset + 20..table_offset + 24]
        .copy_from_slice(&u32::try_from(raw_offset).unwrap().to_le_bytes());
    bytes.extend_from_slice(&[1, 2, 3]);
    for end in 0..table_offset {
        assert!(matches!(
            parse_pe_sections(&bytes[..end]),
            Err(PeHeaderError::OutOfBounds { .. })
        ));
    }
    for end in table_offset..raw_offset {
        assert_eq!(
            parse_pe_sections(&bytes[..end]),
            Err(PeHeaderError::SectionTableOutOfBounds {
                offset: FileOffset::new(table_offset as u64),
                needed: 80,
                available: (end - table_offset) as u64,
            })
        );
    }
    for end in raw_offset..bytes.len() {
        assert_eq!(
            parse_pe_sections(&bytes[..end]),
            Err(PeHeaderError::SectionRawDataOutOfBounds {
                section_index: 0,
                section_offset: FileOffset::new(table_offset as u64),
                offset: FileOffset::new(raw_offset as u64),
                needed: 3,
                available: (end - raw_offset) as u64,
            })
        );
    }
    assert_eq!(
        parse_pe_sections(&bytes).unwrap().sections[0].raw_data,
        &[1, 2, 3]
    );
}

#[test]
fn nonempty_extreme_raw_ranges_use_wide_file_bounds() {
    for (pointer, size) in [(u32::MAX, 2), (1, u32::MAX)] {
        let mut bytes = fixture(false, 96, 1);
        let table_offset = OPTIONAL_OFFSET + 96;
        bytes[table_offset + 16..table_offset + 20].copy_from_slice(&size.to_le_bytes());
        bytes[table_offset + 20..table_offset + 24].copy_from_slice(&pointer.to_le_bytes());
        assert_eq!(
            parse_pe_sections(&bytes),
            Err(PeHeaderError::SectionRawDataOutOfBounds {
                section_index: 0,
                section_offset: FileOffset::new(table_offset as u64),
                offset: FileOffset::new(u64::from(pointer)),
                needed: u64::from(size),
                available: (bytes.len() as u64).saturating_sub(u64::from(pointer)),
            })
        );
    }
}

#[test]
fn empty_raw_data_preserves_its_pointer_without_following_it() {
    for (plus, optional_size) in [(false, 96), (true, 112)] {
        let mut bytes = fixture(plus, optional_size, 1);
        let table_offset = OPTIONAL_OFFSET + usize::from(optional_size);
        bytes[table_offset + 8..table_offset + 12].copy_from_slice(&u32::MAX.to_le_bytes());
        bytes[table_offset + 20..table_offset + 24].copy_from_slice(&u32::MAX.to_le_bytes());
        let table = parse_pe_sections(&bytes).unwrap();
        let section = &table.sections[0];
        assert_eq!(section.virtual_size, u32::MAX);
        assert_eq!(
            section.pointer_to_raw_data,
            FileOffset::new(u64::from(u32::MAX))
        );
        assert!(section.raw_data.is_empty());
        assert!(std::ptr::eq(section.raw_data.as_ptr(), bytes.as_ptr()));
    }
}

#[test]
fn unequal_raw_and_virtual_sizes_do_not_generate_or_discard_bytes() {
    for virtual_size in [0_u32, 2, 8, u32::MAX] {
        let mut bytes = fixture(false, 96, 1);
        let table_offset = OPTIONAL_OFFSET + 96;
        let raw_offset = bytes.len();
        bytes[table_offset + 8..table_offset + 12].copy_from_slice(&virtual_size.to_le_bytes());
        bytes[table_offset + 16..table_offset + 20].copy_from_slice(&4_u32.to_le_bytes());
        bytes[table_offset + 20..table_offset + 24]
            .copy_from_slice(&u32::try_from(raw_offset).unwrap().to_le_bytes());
        bytes.extend_from_slice(&[1, 2, 3, 4]);
        let table = parse_pe_sections(&bytes).unwrap();
        assert_eq!(table.sections[0].virtual_size, virtual_size);
        assert_eq!(table.sections[0].size_of_raw_data, 4);
        assert_eq!(table.sections[0].raw_data, &[1, 2, 3, 4]);
    }
}

#[test]
fn unsorted_and_overlapping_metadata_stays_in_input_order() {
    let mut bytes = fixture(false, 96, 3);
    let table_offset = OPTIONAL_OFFSET + 96;
    for (index, virtual_address) in [0x2000_u32, 0x1000, 0x1000].into_iter().enumerate() {
        let record = &mut bytes[table_offset + index * 40..table_offset + (index + 1) * 40];
        record[0] = u8::try_from(index).unwrap();
        record[8..12].copy_from_slice(&8_u32.to_le_bytes());
        record[12..16].copy_from_slice(&virtual_address.to_le_bytes());
        record[16..20].copy_from_slice(&2_u32.to_le_bytes());
    }
    let table = parse_pe_sections(&bytes).unwrap();
    for (index, virtual_address) in [0x2000_u32, 0x1000, 0x1000].into_iter().enumerate() {
        assert_eq!(table.sections[index].name[0], u8::try_from(index).unwrap());
        assert_eq!(
            table.sections[index].virtual_address,
            RelativeVirtualAddress::new(virtual_address)
        );
        assert_eq!(table.sections[index].raw_data, b"MZ");
    }
}

#[test]
fn a_later_bad_section_returns_failure_instead_of_partial_success() {
    let mut bytes = fixture(false, 96, 2);
    let second_offset = OPTIONAL_OFFSET + 96 + 40;
    bytes[second_offset + 16..second_offset + 20].copy_from_slice(&1_u32.to_le_bytes());
    bytes[second_offset + 20..second_offset + 24].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(
        parse_pe_sections(&bytes),
        Err(PeHeaderError::SectionRawDataOutOfBounds {
            section_index: 1,
            section_offset: FileOffset::new(second_offset as u64),
            offset: FileOffset::new(u64::from(u32::MAX)),
            needed: 1,
            available: 0,
        })
    );
}

fn check_recorded_section(path_variable: &str, optional_size: u16) {
    let path = std::env::var_os(path_variable).expect("the explicit fixture path is required");
    let bytes = std::fs::read(path).expect("the generated fixture must be readable");
    assert_eq!(bytes.len(), 1024);
    let table = parse_pe_sections(&bytes).unwrap();
    assert_eq!(table.headers.prefix.size_of_optional_header, optional_size);
    assert_eq!(table.sections.len(), 1);
    let section = &table.sections[0];
    assert_eq!(section.name, *b".text\0\0\0");
    assert_eq!(section.virtual_size, 11);
    assert_eq!(section.virtual_address, RelativeVirtualAddress::new(0x1000));
    assert_eq!(section.size_of_raw_data, 512);
    assert_eq!(section.pointer_to_raw_data, FileOffset::new(512));
    assert_eq!(section.pointer_to_relocations, FileOffset::new(0));
    assert_eq!(section.pointer_to_line_numbers, FileOffset::new(0));
    assert_eq!(section.number_of_relocations, 0);
    assert_eq!(section.number_of_line_numbers, 0);
    assert_eq!(section.characteristics, 0x6000_0020);
    assert_eq!(section.raw_data, &bytes[512..1024]);
    assert!(std::ptr::eq(
        section.raw_data.as_ptr(),
        bytes[512..].as_ptr()
    ));
    assert_eq!(
        &section.raw_data[..11],
        &[0xb8, 7, 0, 0, 0, 0x83, 0xc0, 0x23, 0xcc, 0x0f, 0x0b]
    );
}

#[test]
#[ignore = "requires the explicit RING3_PE32_FIXTURE path"]
fn generated_pe32_section_matches_recorded_bytes() {
    check_recorded_section("RING3_PE32_FIXTURE", 224);
}

#[test]
#[ignore = "requires the explicit RING3_PE32PLUS_FIXTURE path"]
fn disposable_pe32_plus_section_matches_recorded_bytes() {
    check_recorded_section("RING3_PE32PLUS_FIXTURE", 240);
}
