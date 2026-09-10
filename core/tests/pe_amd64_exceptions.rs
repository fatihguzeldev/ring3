use ring3_core::{
    FileOffset, PeAmd64ExceptionEntry, PeAmd64ExceptionError, PeAmd64ExceptionTable, PeHeaderError,
    PeKind, PeRvaError, RelativeVirtualAddress, parse_pe_amd64_exception_functions,
};

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn directory(bytes: &mut [u8], rva: u32, size: u32) {
    put32(bytes, 288, rva);
    put32(bytes, 292, size);
}

fn fixture() -> Vec<u8> {
    let mut bytes = vec![0; 65_536];
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 60, 128);
    bytes[128..132].copy_from_slice(b"PE\0\0");
    bytes[132..134].copy_from_slice(&0x8664_u16.to_le_bytes());
    bytes[134..136].copy_from_slice(&1_u16.to_le_bytes());
    bytes[148..150].copy_from_slice(&144_u16.to_le_bytes());
    bytes[152..154].copy_from_slice(&0x20b_u16.to_le_bytes());
    put32(&mut bytes, 212, 512);
    put32(&mut bytes, 260, 4);
    bytes[296..304].copy_from_slice(b"opaque\0\0");
    for (offset, value) in [(304, 60_000), (308, 4096), (312, 60_000), (316, 512)] {
        put32(&mut bytes, offset, value);
    }
    directory(&mut bytes, 4096, 12);
    bytes
}

fn record(bytes: &mut [u8], offset: usize, values: [u32; 3]) {
    for (index, value) in values.into_iter().enumerate() {
        put32(bytes, offset + index * 4, value);
    }
}

fn expected(index: u32, rva: u32, file: u64, values: [u32; 3]) -> PeAmd64ExceptionEntry {
    PeAmd64ExceptionEntry {
        table_index: index,
        entry_rva: RelativeVirtualAddress::new(rva),
        entry_file_offset: FileOffset::new(file),
        begin_rva: RelativeVirtualAddress::new(values[0]),
        end_rva: RelativeVirtualAddress::new(values[1]),
        unwind_info_rva: RelativeVirtualAddress::new(values[2]),
    }
}

#[test]
fn raw_order_zero_duplicates_reversed_ranges_and_unread_targets_are_preserved() {
    let mut bytes = fixture();
    directory(&mut bytes, 4096, 60);
    let rows = [
        [9, 1, u32::MAX],
        [0; 3],
        [9, 1, u32::MAX],
        [2, 2, 1],
        [1, 9, 3],
    ];
    let mut entries = Vec::new();
    for (index, row) in rows.into_iter().enumerate() {
        record(&mut bytes, 512 + index * 12, row);
        let index = u32::try_from(index).unwrap();
        entries.push(expected(
            index,
            4096 + index * 12,
            512 + u64::from(index) * 12,
            row,
        ));
    }
    let before = bytes.clone();
    let result = parse_pe_amd64_exception_functions(&bytes);
    assert_eq!(bytes, before);
    bytes.fill(0);
    drop(bytes);
    assert_eq!(
        result,
        Ok(Some(PeAmd64ExceptionTable {
            directory_rva: RelativeVirtualAddress::new(4096),
            directory_file_offset: FileOffset::new(512),
            directory_size: 60,
            entries,
        }))
    );
}

#[test]
fn header_backing_and_odd_physical_offsets_are_supported() {
    for (rva, file) in [(400, 400), (4096, 513)] {
        let mut bytes = fixture();
        directory(&mut bytes, rva, 12);
        put32(&mut bytes, 316, 513);
        record(&mut bytes, file, [u32::MAX, 0, 1]);
        let table = parse_pe_amd64_exception_functions(&bytes).unwrap().unwrap();
        assert_eq!(table.directory_file_offset, FileOffset::new(file as u64));
        assert_eq!(
            table.entries,
            [expected(0, rva, file as u64, [u32::MAX, 0, 1])]
        );
    }
}

#[test]
fn absent_slots_precede_the_present_image_gate_for_both_widths() {
    for (machine, kind) in [
        (0x8664, PeKind::Pe32Plus),
        (0xaa64, PeKind::Pe32Plus),
        (0x9999, PeKind::Pe32Plus),
        (0x14c, PeKind::Pe32),
        (0x9999, PeKind::Pe32),
    ] {
        let mut bytes = fixture();
        bytes[132..134].copy_from_slice(&u16::try_from(machine).unwrap().to_le_bytes());
        if kind == PeKind::Pe32 {
            bytes[152..154].copy_from_slice(&0x10b_u16.to_le_bytes());
            put32(&mut bytes, 244, 4);
            put32(&mut bytes, 272, 0);
            put32(&mut bytes, 276, 0);
        }
        directory(&mut bytes, 0, 0);
        assert_eq!(parse_pe_amd64_exception_functions(&bytes), Ok(None));
        let (count_offset, directory_offset) = if kind == PeKind::Pe32 {
            (244, 272)
        } else {
            (260, 288)
        };
        put32(&mut bytes, count_offset, 3);
        put32(&mut bytes, directory_offset, u32::MAX);
        put32(&mut bytes, directory_offset + 4, u32::MAX);
        assert_eq!(parse_pe_amd64_exception_functions(&bytes), Ok(None));
    }
}

#[test]
fn complete_base_validation_precedes_absence() {
    let mut bytes = fixture();
    directory(&mut bytes, 0, 0);
    bytes[0] = 0;
    assert_eq!(
        parse_pe_amd64_exception_functions(&bytes),
        Err(PeAmd64ExceptionError::Base(PeRvaError::Parse(
            PeHeaderError::InvalidDosSignature {
                offset: FileOffset::new(0)
            }
        )))
    );
    bytes[0] = b'M';
    put32(&mut bytes, 212, 1);
    assert_eq!(
        parse_pe_amd64_exception_functions(&bytes),
        Err(PeAmd64ExceptionError::Base(
            PeRvaError::InvalidHeaderExtent {
                size_of_headers: 1,
                minimum: 336,
                file_size: 65_536
            }
        ))
    );
}

#[test]
fn consistency_and_coordinate_overflow_precede_machine_and_alignment() {
    let mut bytes = fixture();
    bytes[132..134].copy_from_slice(&0xaa64_u16.to_le_bytes());
    for (rva, size) in [(0, 13), (4097, 0)] {
        directory(&mut bytes, rva, size);
        assert_eq!(
            parse_pe_amd64_exception_functions(&bytes),
            Err(PeAmd64ExceptionError::InconsistentDirectory {
                rva: RelativeVirtualAddress::new(rva),
                size,
            })
        );
    }
    directory(&mut bytes, u32::MAX, 13);
    assert_eq!(
        parse_pe_amd64_exception_functions(&bytes),
        Err(PeAmd64ExceptionError::DirectoryRangeOverflow {
            rva: RelativeVirtualAddress::new(u32::MAX),
            size: 13,
        })
    );
    directory(&mut bytes, 4097, 13);
    assert_eq!(
        parse_pe_amd64_exception_functions(&bytes),
        Err(PeAmd64ExceptionError::UnsupportedImage {
            machine: 0xaa64,
            kind: PeKind::Pe32Plus,
        })
    );
}

#[test]
fn alignment_size_and_count_precede_backing() {
    let mut bytes = fixture();
    directory(&mut bytes, 0x20001, 13);
    assert_eq!(
        parse_pe_amd64_exception_functions(&bytes),
        Err(PeAmd64ExceptionError::UnalignedDirectory {
            rva: RelativeVirtualAddress::new(0x20001),
        })
    );
    directory(&mut bytes, 0x20000, 49_153);
    assert_eq!(
        parse_pe_amd64_exception_functions(&bytes),
        Err(PeAmd64ExceptionError::InvalidDirectorySize { size: 49_153 })
    );
    directory(&mut bytes, 0x20000, 49_164);
    assert_eq!(
        parse_pe_amd64_exception_functions(&bytes),
        Err(PeAmd64ExceptionError::EntryLimitExceeded {
            count: 4097,
            limit: 4096
        })
    );
    directory(&mut bytes, 4096, 49_152);
    let table = parse_pe_amd64_exception_functions(&bytes).unwrap().unwrap();
    assert_eq!(table.entries.len(), 4096);
    assert_eq!(table.entries[4095], expected(4095, 53_236, 49_652, [0; 3]));
}

#[test]
fn coordinate_end_exactly_two_to_the_32_is_supported() {
    let mut bytes = fixture();
    directory(&mut bytes, 0xffff_fff4, 12);
    for (offset, value) in [(304, 12), (308, 0xffff_fff4), (312, 12)] {
        put32(&mut bytes, offset, value);
    }
    record(&mut bytes, 512, [u32::MAX, 0, 0xffff_fffd]);
    assert_eq!(
        parse_pe_amd64_exception_functions(&bytes)
            .unwrap()
            .unwrap()
            .entries,
        [expected(0, 0xffff_fff4, 512, [u32::MAX, 0, 0xffff_fffd])]
    );
}

#[test]
fn the_whole_table_must_have_one_conservative_backing_range() {
    let mut bytes = fixture();
    let start = RelativeVirtualAddress::new(4096);
    for (virtual_size, raw_size, cause) in [
        (
            0,
            24,
            PeRvaError::ZeroVirtualSizeUnsupported {
                start,
                length: 24,
                section_index: 0,
            },
        ),
        (
            12,
            24,
            PeRvaError::RawPaddingUnsupported {
                start,
                length: 24,
                section_index: 0,
            },
        ),
        (
            24,
            12,
            PeRvaError::NotFileBacked {
                start,
                length: 24,
                section_index: 0,
            },
        ),
    ] {
        directory(&mut bytes, 4096, 24);
        put32(&mut bytes, 304, virtual_size);
        put32(&mut bytes, 312, raw_size);
        assert_eq!(
            parse_pe_amd64_exception_functions(&bytes),
            Err(PeAmd64ExceptionError::DirectoryRange {
                start,
                length: 24,
                cause
            })
        );
    }
    directory(&mut bytes, 504, 24);
    assert_eq!(
        parse_pe_amd64_exception_functions(&bytes),
        Err(PeAmd64ExceptionError::DirectoryRange {
            start: RelativeVirtualAddress::new(504),
            length: 24,
            cause: PeRvaError::CrossesRegionBoundary {
                start: RelativeVirtualAddress::new(504),
                length: 24
            },
        })
    );
    directory(&mut bytes, 4096, 24);
    bytes[134..136].copy_from_slice(&2_u16.to_le_bytes());
    for (offset, value) in [(344, 24), (348, 4096), (352, 24), (356, 1024)] {
        put32(&mut bytes, offset, value);
    }
    assert_eq!(
        parse_pe_amd64_exception_functions(&bytes),
        Err(PeAmd64ExceptionError::DirectoryRange {
            start,
            length: 24,
            cause: PeRvaError::AmbiguousRange { start, length: 24 },
        })
    );
    put32(&mut bytes, 304, 12);
    put32(&mut bytes, 348, 4108);
    assert_eq!(
        parse_pe_amd64_exception_functions(&bytes),
        Err(PeAmd64ExceptionError::DirectoryRange {
            start,
            length: 24,
            cause: PeRvaError::CrossesRegionBoundary { start, length: 24 },
        })
    );
}

#[test]
#[ignore = "requires the self-authored linked AMD64 exception fixture"]
fn generated_amd64_exception_functions_preserve_raw_records() {
    let path = std::env::var("RING3_EXCEPTION_AMD64_FIXTURE")
        .expect("RING3_EXCEPTION_AMD64_FIXTURE is required");
    let bytes = std::fs::read(path).expect("read the AMD64 exception fixture");
    assert_eq!(bytes.len(), 2560);
    let headers = ring3_core::parse_pe_headers(&bytes).unwrap();
    assert_eq!(headers.prefix.kind, PeKind::Pe32Plus);
    assert_eq!(headers.prefix.machine, 0x8664);
    assert_eq!(
        parse_pe_amd64_exception_functions(&bytes),
        Ok(Some(PeAmd64ExceptionTable {
            directory_rva: RelativeVirtualAddress::new(12288),
            directory_file_offset: FileOffset::new(2048),
            directory_size: 24,
            entries: vec![
                expected(0, 12288, 2048, [4096, 4110, 8288]),
                expected(1, 12300, 2060, [4112, 4130, 8296]),
            ],
        }))
    );
}

#[test]
#[ignore = "requires the self-authored linked AMD64 leaf fixture"]
fn generated_amd64_leaf_image_has_no_exception_functions() {
    let path = std::env::var("RING3_EXCEPTION_AMD64_LEAF_FIXTURE")
        .expect("RING3_EXCEPTION_AMD64_LEAF_FIXTURE is required");
    let bytes = std::fs::read(path).expect("read the AMD64 leaf fixture");
    assert_eq!(bytes.len(), 2048);
    let headers = ring3_core::parse_pe_headers(&bytes).unwrap();
    assert_eq!(headers.prefix.kind, PeKind::Pe32Plus);
    assert_eq!(headers.prefix.machine, 0x8664);
    assert_eq!(parse_pe_amd64_exception_functions(&bytes), Ok(None));
}
