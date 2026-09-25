use ring3_core::{
    FileOffset, PeHeaderError, PeImportDescriptor, PeImportError, PeRvaError,
    RelativeVirtualAddress, parse_pe_import_descriptors,
};

fn put_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn directory(bytes: &mut [u8], plus: bool, rva: u32, size: u32) {
    let slot = 152 + if plus { 112 } else { 96 } + 8;
    put_u32(bytes, slot, rva);
    put_u32(bytes, slot + 4, size);
}

fn record(bytes: &mut [u8], index: usize, fields: [u32; 5]) {
    for (field, value) in fields.into_iter().enumerate() {
        put_u32(bytes, 512 + index * 20 + field * 4, value);
    }
}

fn section(
    bytes: &mut [u8],
    index: usize,
    va: u32,
    virtual_size: u32,
    raw_size: u32,
    pointer: u32,
) {
    for (offset, value) in [(8, virtual_size), (12, va), (16, raw_size), (20, pointer)] {
        put_u32(bytes, 264 + index * 40 + offset, value);
    }
}

fn fixture(plus: bool, count: u16) -> Vec<u8> {
    let mut bytes = vec![0; 8192];
    bytes[..2].copy_from_slice(b"MZ");
    put_u32(&mut bytes, 60, 128);
    bytes[128..132].copy_from_slice(b"PE\0\0");
    bytes[134..136].copy_from_slice(&2_u16.to_le_bytes());
    let fixed = if plus { 112_u16 } else { 96 };
    bytes[148..150].copy_from_slice(&(fixed + 16).to_le_bytes());
    bytes[152..154].copy_from_slice(&if plus { 0x20b_u16 } else { 0x10b }.to_le_bytes());
    put_u32(&mut bytes, 212, 512);
    put_u32(&mut bytes, 152 + usize::from(fixed) - 4, 2);
    directory(&mut bytes, plus, 0x1000, (u32::from(count) + 1) * 20);
    let table = 152 + usize::from(fixed) + 16;
    for (index, (rva, size, pointer)) in [(0x1000, 4096, 512), (0x3000, 2048, 4608)]
        .into_iter()
        .enumerate()
    {
        for (offset, value) in [(8, size), (12, rva), (16, size), (20, pointer)] {
            put_u32(&mut bytes, table + index * 40 + offset, value);
        }
    }
    for index in 0..count {
        record(
            &mut bytes,
            usize::from(index),
            [0x4000, 123, 456, 0x3000, 0x5000],
        );
    }
    bytes[4608..4616].copy_from_slice(b"Own.DLL\0");
    bytes
}

#[test]
fn both_layouts_preserve_fields_order_case_duplicates_and_borrowing() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, 3);
        record(
            &mut bytes,
            0,
            [0, 0x8765_4321, 0x1234_5678, 0x3000, 0xfedc_ba98],
        );
        record(&mut bytes, 1, [1, 2, 3, 0x3010, 4]);
        bytes[4624..4636].copy_from_slice(b"Other\t.DLL\x01\0");
        let descriptors = parse_pe_import_descriptors(&bytes).unwrap();
        assert_eq!(descriptors.len(), 3);
        assert_eq!(
            descriptors[0].descriptor_rva,
            RelativeVirtualAddress::new(0x1000)
        );
        assert_eq!(descriptors[0].descriptor_file_offset, FileOffset::new(512));
        assert_eq!(
            descriptors[0].import_lookup_table_rva,
            RelativeVirtualAddress::new(0)
        );
        assert_eq!(descriptors[0].time_date_stamp, 0x8765_4321);
        assert_eq!(descriptors[0].forwarder_chain, 0x1234_5678);
        assert_eq!(descriptors[0].name_rva, RelativeVirtualAddress::new(0x3000));
        assert_eq!(
            descriptors[0].import_address_table_rva,
            RelativeVirtualAddress::new(0xfedc_ba98)
        );
        assert_eq!(descriptors[0].dll_name, "Own.DLL");
        assert_eq!(
            descriptors[1].descriptor_rva,
            RelativeVirtualAddress::new(0x1014)
        );
        assert_eq!(descriptors[1].descriptor_file_offset, FileOffset::new(532));
        assert_eq!(descriptors[1].dll_name, "Other\t.DLL\x01");
        assert_eq!(descriptors[2].dll_name, descriptors[0].dll_name);
        assert!(std::ptr::eq(
            descriptors[0].dll_name.as_ptr(),
            bytes[4608..].as_ptr()
        ));
    }
}

#[test]
fn complete_terminator_is_required_but_trailing_declared_bytes_are_ignored() {
    let mut bytes = fixture(false, 1);
    directory(&mut bytes, false, 0x1000, 20);
    assert_eq!(
        parse_pe_import_descriptors(&bytes),
        Err(PeImportError::MissingImportTerminator {
            descriptor_index: 1
        })
    );
    directory(&mut bytes, false, 0x1000, 39);
    assert_eq!(
        parse_pe_import_descriptors(&bytes),
        Err(PeImportError::TruncatedImportDescriptor {
            descriptor_index: 1,
            remaining: 19
        })
    );
    directory(&mut bytes, false, 0x1000, 0x0010_0001);
    record(&mut bytes, 2, [1, 2, 3, u32::MAX, 4]);
    assert_eq!(parse_pe_import_descriptors(&bytes).unwrap().len(), 1);
}

#[test]
fn every_descriptor_field_participates_in_the_all_zero_terminator() {
    for field in 0..5 {
        let mut bytes = fixture(false, 1);
        let mut fields = [0; 5];
        fields[field] = if field == 3 { 0x3000 } else { 1 };
        record(&mut bytes, 0, fields);
        let descriptors = parse_pe_import_descriptors(&bytes).unwrap();
        assert_eq!(descriptors.len(), 1);
        assert_eq!(
            descriptors[0].dll_name,
            if field == 3 { "Own.DLL" } else { "MZ" }
        );
    }
}

#[test]
fn absent_and_empty_directories_still_validate_the_base_file() {
    let mut absent = fixture(false, 0);
    put_u32(&mut absent, 244, 1);
    assert!(parse_pe_import_descriptors(&absent).unwrap().is_empty());
    let mut empty = fixture(false, 0);
    directory(&mut empty, false, 0, 0);
    assert!(parse_pe_import_descriptors(&empty).unwrap().is_empty());
    for mut bytes in [absent, empty] {
        put_u32(&mut bytes, 212, 343);
        assert_eq!(
            parse_pe_import_descriptors(&bytes),
            Err(PeImportError::Base(PeRvaError::InvalidHeaderExtent {
                size_of_headers: 343,
                minimum: 344,
                file_size: 8192,
            }))
        );
        bytes[0] = 0;
        assert_eq!(
            parse_pe_import_descriptors(&bytes),
            Err(PeImportError::Base(PeRvaError::Parse(
                PeHeaderError::InvalidDosSignature {
                    offset: FileOffset::new(0)
                }
            )))
        );
    }
    for (rva, size) in [(0, 20), (0x1000, 0)] {
        let mut bytes = fixture(false, 0);
        directory(&mut bytes, false, rva, size);
        assert_eq!(
            parse_pe_import_descriptors(&bytes),
            Err(PeImportError::InconsistentImportDirectory {
                rva: RelativeVirtualAddress::new(rva),
                size
            })
        );
    }
}

#[test]
fn declared_directory_end_accepts_the_last_rva_byte_without_wrapping() {
    let mut bytes = fixture(false, 0);
    let rva = u32::MAX - 19;
    section(&mut bytes, 0, rva, 20, 20, 512);
    directory(&mut bytes, false, rva, 20);
    assert!(parse_pe_import_descriptors(&bytes).unwrap().is_empty());
    directory(&mut bytes, false, rva, 21);
    assert_eq!(
        parse_pe_import_descriptors(&bytes),
        Err(PeImportError::ImportDirectoryRangeOverflow {
            rva: RelativeVirtualAddress::new(rva),
            size: 21
        })
    );
    directory(&mut bytes, false, 0x1000, 19);
    assert_eq!(
        parse_pe_import_descriptors(&bytes),
        Err(PeImportError::TruncatedImportDescriptor {
            descriptor_index: 0,
            remaining: 19
        })
    );
}

#[test]
fn descriptor_limit_inspects_the_129th_record_before_following_its_name() {
    assert_eq!(
        parse_pe_import_descriptors(&fixture(false, 128))
            .unwrap()
            .len(),
        128
    );
    let mut bytes = fixture(false, 129);
    record(&mut bytes, 128, [0, 0, 0, u32::MAX, 0]);
    assert_eq!(
        parse_pe_import_descriptors(&bytes),
        Err(PeImportError::ImportDescriptorLimitExceeded {
            descriptor_index: 128,
            limit: 128
        })
    );
    directory(&mut bytes, false, 0x1000, 2560);
    assert_eq!(
        parse_pe_import_descriptors(&bytes),
        Err(PeImportError::MissingImportTerminator {
            descriptor_index: 128
        })
    );
    directory(&mut bytes, false, 0x1000, 2579);
    assert_eq!(
        parse_pe_import_descriptors(&bytes),
        Err(PeImportError::TruncatedImportDescriptor {
            descriptor_index: 128,
            remaining: 19
        })
    );
    directory(&mut bytes, false, 0x1000, 2580);
    section(&mut bytes, 0, 0x1000, 2580, 2560, 512);
    let start = RelativeVirtualAddress::new(0x1000);
    assert_eq!(
        parse_pe_import_descriptors(&bytes),
        Err(PeImportError::DescriptorRange {
            descriptor_index: 128,
            start,
            length: 2580,
            cause: PeRvaError::NotFileBacked {
                start,
                length: 2580,
                section_index: 0
            },
        })
    );
}

#[test]
fn table_prefixes_refuse_stitching_padding_zero_fill_and_ambiguity() {
    let start = RelativeVirtualAddress::new(0x1000);
    for (virtual_size, raw_size, cause) in [
        (
            20,
            40,
            PeRvaError::RawPaddingUnsupported {
                start,
                length: 40,
                section_index: 0,
            },
        ),
        (
            40,
            20,
            PeRvaError::NotFileBacked {
                start,
                length: 40,
                section_index: 0,
            },
        ),
    ] {
        let mut bytes = fixture(false, 1);
        section(&mut bytes, 0, 0x1000, virtual_size, raw_size, 512);
        assert_eq!(
            parse_pe_import_descriptors(&bytes),
            Err(PeImportError::DescriptorRange {
                descriptor_index: 1,
                start,
                length: 40,
                cause
            })
        );
    }
    for (first_size, cause) in [
        (20, PeRvaError::CrossesRegionBoundary { start, length: 40 }),
        (40, PeRvaError::AmbiguousRange { start, length: 40 }),
    ] {
        let mut bytes = fixture(false, 1);
        record(&mut bytes, 0, [0, 0, 0, 384, 0]);
        bytes[384..386].copy_from_slice(b"A\0");
        section(&mut bytes, 0, 0x1000, first_size, first_size, 512);
        section(&mut bytes, 1, 0x1014, 20, 20, 532);
        assert_eq!(
            parse_pe_import_descriptors(&bytes),
            Err(PeImportError::DescriptorRange {
                descriptor_index: 1,
                start,
                length: 40,
                cause
            })
        );
    }
}

#[test]
fn name_prefixes_refuse_stitching_padding_zero_fill_and_ambiguity() {
    let name_rva = RelativeVirtualAddress::new(0x3000);
    for (virtual_size, raw_size, cause) in [
        (
            3,
            5,
            PeRvaError::RawPaddingUnsupported {
                start: name_rva,
                length: 4,
                section_index: 1,
            },
        ),
        (
            5,
            3,
            PeRvaError::NotFileBacked {
                start: name_rva,
                length: 4,
                section_index: 1,
            },
        ),
    ] {
        let mut bytes = fixture(false, 1);
        bytes[4608..4613].copy_from_slice(b"ABCD\0");
        section(&mut bytes, 1, 0x3000, virtual_size, raw_size, 4608);
        assert_eq!(
            parse_pe_import_descriptors(&bytes),
            Err(PeImportError::NameRange {
                descriptor_index: 0,
                name_rva,
                offset: 3,
                cause
            })
        );
    }
    for (first_size, cause) in [
        (
            3,
            PeRvaError::CrossesRegionBoundary {
                start: name_rva,
                length: 4,
            },
        ),
        (
            5,
            PeRvaError::AmbiguousRange {
                start: name_rva,
                length: 4,
            },
        ),
    ] {
        let mut bytes = fixture(false, 1);
        bytes[134..136].copy_from_slice(&3_u16.to_le_bytes());
        bytes[4608..4613].copy_from_slice(b"ABCD\0");
        section(&mut bytes, 1, 0x3000, first_size, first_size, 4608);
        section(&mut bytes, 2, 0x3003, 2, 2, 4611);
        assert_eq!(
            parse_pe_import_descriptors(&bytes),
            Err(PeImportError::NameRange {
                descriptor_index: 0,
                name_rva,
                offset: 3,
                cause
            })
        );
    }
}

#[test]
fn name_errors_report_the_first_offending_byte_without_returning_partial_results() {
    let name_rva = RelativeVirtualAddress::new(0x3010);
    let mut bytes = fixture(false, 2);
    record(&mut bytes, 1, [0, 0, 0, name_rva.get(), 0]);
    assert_eq!(
        parse_pe_import_descriptors(&bytes),
        Err(PeImportError::EmptyDllName {
            descriptor_index: 1,
            name_rva
        })
    );
    bytes[4624..4627].copy_from_slice(&[b'A', 0x80, 0]);
    assert_eq!(
        parse_pe_import_descriptors(&bytes),
        Err(PeImportError::NonAsciiDllName {
            descriptor_index: 1,
            name_rva,
            offset: 1,
            byte: 0x80
        })
    );
    let mut edge = fixture(false, 1);
    record(&mut edge, 0, [0, 0, 0, u32::MAX, 0]);
    section(&mut edge, 1, u32::MAX, 1, 1, 4608);
    let name_rva = RelativeVirtualAddress::new(u32::MAX);
    assert_eq!(
        parse_pe_import_descriptors(&edge),
        Err(PeImportError::NameRange {
            descriptor_index: 0,
            name_rva,
            offset: 1,
            cause: PeRvaError::RvaRangeOverflow {
                start: name_rva,
                length: 2
            },
        })
    );
}

#[test]
fn per_name_limit_counts_nul_and_precedes_an_unread_boundary() {
    let mut bytes = fixture(false, 1);
    bytes[4608..5631].fill(b'A');
    bytes[5631] = 0;
    section(&mut bytes, 1, 0x3000, 1024, 1024, 4608);
    assert_eq!(
        parse_pe_import_descriptors(&bytes).unwrap()[0]
            .dll_name
            .len(),
        1023
    );
    bytes[5631] = b'A';
    assert_eq!(
        parse_pe_import_descriptors(&bytes),
        Err(PeImportError::NameLengthLimitExceeded {
            descriptor_index: 0,
            name_rva: RelativeVirtualAddress::new(0x3000),
            limit: 1024
        })
    );
}

#[test]
fn total_budget_counts_duplicate_scans_and_stops_before_the_next_byte() {
    let mut bytes = fixture(false, 64);
    bytes[4608..5631].fill(b'A');
    bytes[5631] = 0;
    assert_eq!(parse_pe_import_descriptors(&bytes).unwrap().len(), 64);
    directory(&mut bytes, false, 0x1000, 66 * 20);
    record(&mut bytes, 64, [0, 0, 0, 0x3000, 0]);
    let name_rva = RelativeVirtualAddress::new(0x3000);
    assert_eq!(
        parse_pe_import_descriptors(&bytes),
        Err(PeImportError::NameScanBudgetExceeded {
            descriptor_index: 64,
            name_rva,
            offset: 0,
            limit: 65_536
        })
    );
    record(&mut bytes, 63, [0, 0, 0, 0x3400, 0]);
    bytes[5632..6143].fill(b'C');
    bytes[6143] = 0;
    assert_eq!(
        parse_pe_import_descriptors(&bytes),
        Err(PeImportError::NameScanBudgetExceeded {
            descriptor_index: 64,
            name_rva,
            offset: 512,
            limit: 65_536
        })
    );
}

#[test]
fn simultaneous_name_and_total_exhaustion_reports_the_name_limit_first() {
    let mut bytes = fixture(false, 64);
    bytes[4608..5631].fill(b'A');
    bytes[5631] = 0;
    bytes[5632..6656].fill(b'B');
    record(&mut bytes, 63, [0, 0, 0, 0x3400, 0]);
    assert_eq!(
        parse_pe_import_descriptors(&bytes),
        Err(PeImportError::NameLengthLimitExceeded {
            descriptor_index: 63,
            name_rva: RelativeVirtualAddress::new(0x3400),
            limit: 1024
        })
    );
}

fn check_import_fixture(variable: &str, name_rva: u32, iat_rva: u32, name_offset: usize) {
    let path =
        std::env::var(variable).expect("an explicit generated import fixture path is required");
    let bytes = std::fs::read(path).unwrap();
    let descriptors = parse_pe_import_descriptors(&bytes).unwrap();
    assert_eq!(
        descriptors,
        [PeImportDescriptor {
            descriptor_rva: RelativeVirtualAddress::new(8192),
            descriptor_file_offset: FileOffset::new(1536),
            import_lookup_table_rva: RelativeVirtualAddress::new(8232),
            time_date_stamp: 0,
            forwarder_chain: 0,
            name_rva: RelativeVirtualAddress::new(name_rva),
            import_address_table_rva: RelativeVirtualAddress::new(iat_rva),
            dll_name: "Ring3Probe.dll",
        }]
    );
    assert!(std::ptr::eq(
        descriptors[0].dll_name.as_ptr(),
        bytes[name_offset..].as_ptr()
    ));
}

#[test]
#[ignore = "requires the explicit RING3_IMPORT_PE32_FIXTURE path"]
fn generated_pe32_imports_match_recorded_llvm_and_raw_metadata() {
    check_import_fixture("RING3_IMPORT_PE32_FIXTURE", 8262, 8240, 1606);
}

#[test]
#[ignore = "requires the explicit RING3_IMPORT_PE32PLUS_FIXTURE path"]
fn generated_pe32plus_imports_match_recorded_llvm_and_raw_metadata() {
    check_import_fixture("RING3_IMPORT_PE32PLUS_FIXTURE", 8278, 8248, 1622);
}
