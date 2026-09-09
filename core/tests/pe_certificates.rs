use ring3_core::{
    FileOffset, PeCertificateError, PeCertificateTable, PeHeaderError, PeKind,
    parse_pe_certificate_table,
};

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn fixed(plus: bool) -> usize {
    if plus { 112 } else { 96 }
}

fn directory(bytes: &mut [u8], plus: bool, offset: u32, size: u32) {
    put32(bytes, 152 + fixed(plus) + 32, offset);
    put32(bytes, 152 + fixed(plus) + 36, size);
}

fn fixture(plus: bool) -> Vec<u8> {
    let mut bytes = vec![0; 1024];
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 60, 128);
    bytes[128..132].copy_from_slice(b"PE\0\0");
    bytes[134..136].copy_from_slice(&1_u16.to_le_bytes());
    let size = u16::try_from(fixed(plus) + 40).unwrap();
    bytes[148..150].copy_from_slice(&size.to_le_bytes());
    bytes[152..154].copy_from_slice(&if plus { 0x20b_u16 } else { 0x10b }.to_le_bytes());
    put32(&mut bytes, 212, 512);
    put32(&mut bytes, 152 + fixed(plus) - 4, 5);
    let section = 152 + fixed(plus) + 40;
    for (offset, value) in [8, 12, 16, 20].into_iter().zip([16, 512, 16, 768]) {
        put32(&mut bytes, section + offset, value);
    }
    directory(&mut bytes, plus, 512, 16);
    bytes[512..528].copy_from_slice(b"opaque raw bytes");
    bytes[768..784].fill(0xff);
    bytes
}

#[test]
fn both_widths_borrow_file_bytes_without_rva_translation() {
    for plus in [false, true] {
        let bytes = fixture(plus);
        let before = bytes.clone();
        let table = parse_pe_certificate_table(&bytes).unwrap().unwrap();
        assert_eq!(
            table,
            PeCertificateTable {
                kind: if plus { PeKind::Pe32Plus } else { PeKind::Pe32 },
                file_offset: FileOffset::new(512),
                size: 16,
                raw_bytes: b"opaque raw bytes",
            }
        );
        assert_eq!(table.raw_bytes.as_ptr(), bytes[512..].as_ptr());
        assert_eq!(bytes, before);
    }
}

#[test]
fn missing_and_zero_slots_are_absent_but_zero_bytes_are_a_present_table() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        bytes[512..528].fill(0);
        assert_eq!(
            parse_pe_certificate_table(&bytes)
                .unwrap()
                .unwrap()
                .raw_bytes,
            &[0; 16]
        );
        directory(&mut bytes, plus, 0, 0);
        assert_eq!(parse_pe_certificate_table(&bytes), Ok(None));
        directory(&mut bytes, plus, u32::MAX, u32::MAX);
        put32(&mut bytes, 152 + fixed(plus) - 4, 4);
        assert_eq!(parse_pe_certificate_table(&bytes), Ok(None));
    }
}

#[test]
fn header_failure_precedes_absence_and_directory_errors() {
    for plus in [false, true] {
        for (offset, size) in [(0, 0), (0, 7), (513, 0), (513, 7), (512, 4096)] {
            let mut bytes = fixture(plus);
            directory(&mut bytes, plus, offset, size);
            bytes[0] = 0;
            assert_eq!(
                parse_pe_certificate_table(&bytes),
                Err(PeCertificateError::Base(
                    PeHeaderError::InvalidDosSignature {
                        offset: FileOffset::new(0),
                    }
                ))
            );
        }
    }
}

#[test]
fn one_zero_is_inconsistent_before_alignment() {
    for plus in [false, true] {
        for (offset, size) in [(0, 1), (0, 8), (1, 0), (512, 0)] {
            let mut bytes = fixture(plus);
            directory(&mut bytes, plus, offset, size);
            assert_eq!(
                parse_pe_certificate_table(&bytes),
                Err(PeCertificateError::InconsistentDirectory {
                    file_offset: FileOffset::new(u64::from(offset)),
                    size,
                })
            );
        }
    }
}

#[test]
fn offset_alignment_precedes_size_alignment_and_physical_bounds() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        for delta in 1..8 {
            directory(&mut bytes, plus, 1024 + delta, 7);
            assert_eq!(
                parse_pe_certificate_table(&bytes),
                Err(PeCertificateError::UnalignedTableOffset {
                    file_offset: FileOffset::new(u64::from(1024 + delta)),
                })
            );
            directory(&mut bytes, plus, 1024, delta);
            assert_eq!(
                parse_pe_certificate_table(&bytes),
                Err(PeCertificateError::UnalignedTableSize { size: delta })
            );
        }
    }
}

#[test]
fn exact_eof_succeeds_and_missing_bytes_report_full_requested_extent() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        directory(&mut bytes, plus, 1008, 16);
        let table = parse_pe_certificate_table(&bytes).unwrap().unwrap();
        assert_eq!(table.file_offset, FileOffset::new(1008));
        assert_eq!(table.raw_bytes.as_ptr(), bytes[1008..].as_ptr());
        assert_eq!(table.raw_bytes.len(), 16);
        for length in 1008..1024 {
            let truncated = &bytes[..length];
            assert_eq!(
                parse_pe_certificate_table(truncated),
                Err(PeCertificateError::TableRange {
                    file_offset: FileOffset::new(1008),
                    size: 16,
                    cause: PeHeaderError::OutOfBounds {
                        offset: FileOffset::new(1008),
                        needed: 16,
                        available: u64::try_from(length - 1008).unwrap(),
                    },
                })
            );
        }
    }
}

#[test]
fn wide_file_extents_do_not_wrap_into_the_input_or_rva_space() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        for (offset, size, available) in [
            (0xffff_fff8, 16, 0),
            (512, 0xffff_fff8, 512),
            (0xffff_fff8, 0xffff_fff8, 0),
        ] {
            directory(&mut bytes, plus, offset, size);
            assert_eq!(
                parse_pe_certificate_table(&bytes),
                Err(PeCertificateError::TableRange {
                    file_offset: FileOffset::new(u64::from(offset)),
                    size,
                    cause: PeHeaderError::OutOfBounds {
                        offset: FileOffset::new(u64::from(offset)),
                        needed: u64::from(size),
                        available,
                    },
                })
            );
        }
    }
}

#[test]
fn framing_does_not_validate_sections_record_content_or_placement() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        bytes[134..136].copy_from_slice(&u16::MAX.to_le_bytes());
        directory(&mut bytes, plus, 64, 8);
        bytes[64..72].fill(0xff);
        let table = parse_pe_certificate_table(&bytes).unwrap().unwrap();
        assert_eq!(table.file_offset, FileOffset::new(64));
        assert_eq!(table.raw_bytes, &[0xff; 8]);
        assert_eq!(table.raw_bytes.as_ptr(), bytes[64..].as_ptr());
    }
}
