use ring3_core::{FileOffset, PeHeaderError, PeHeaderPrefix, PeKind, parse_pe_header_prefix};

fn fixture(pe_offset: usize, machine: u16, magic: u16, optional_size: u16) -> Vec<u8> {
    let mut bytes = vec![0; (pe_offset + 24 + usize::from(optional_size)).max(64)];
    bytes[0..2].copy_from_slice(b"MZ");
    bytes[0x3c..0x40].copy_from_slice(&u32::try_from(pe_offset).unwrap().to_le_bytes());
    bytes[pe_offset..pe_offset + 4].copy_from_slice(b"PE\0\0");
    bytes[pe_offset + 4..pe_offset + 6].copy_from_slice(&machine.to_le_bytes());
    bytes[pe_offset + 6..pe_offset + 8].copy_from_slice(&u16::MAX.to_le_bytes());
    bytes[pe_offset + 20..pe_offset + 22].copy_from_slice(&optional_size.to_le_bytes());
    bytes[pe_offset + 22..pe_offset + 24].copy_from_slice(&0x1234_u16.to_le_bytes());
    if optional_size >= 2 {
        bytes[pe_offset + 24..pe_offset + 26].copy_from_slice(&magic.to_le_bytes());
    }
    bytes
}

#[test]
fn recognizes_both_layouts_without_classifying_unknown_machines() {
    for (machine, magic, kind) in [
        (0x14c, 0x10b, PeKind::Pe32),
        (0x8664, 0x20b, PeKind::Pe32Plus),
        (0xffff, 0x10b, PeKind::Pe32),
        (0x1c4, 0x20b, PeKind::Pe32Plus),
    ] {
        assert_eq!(
            parse_pe_header_prefix(&fixture(0x80, machine, magic, 2)),
            Ok(PeHeaderPrefix {
                pe_offset: FileOffset::new(0x80),
                machine,
                number_of_sections: u16::MAX,
                characteristics: 0x1234,
                size_of_optional_header: 2,
                kind,
            })
        );
    }
}

#[test]
fn every_required_prefix_truncation_fails_and_trailing_bytes_do_not_matter() {
    let mut bytes = fixture(0x80, 0x14c, 0x10b, 224);
    for end in 0..bytes.len() {
        assert!(matches!(
            parse_pe_header_prefix(&bytes[..end]),
            Err(PeHeaderError::OutOfBounds { .. })
        ));
    }
    let exact = parse_pe_header_prefix(&bytes).unwrap();
    bytes.extend_from_slice(&[0xff; 32]);
    assert_eq!(parse_pe_header_prefix(&bytes), Ok(exact));
}

#[test]
fn rejects_extreme_pe_offset_without_host_width_wrap() {
    let mut bytes = fixture(0x80, 0x14c, 0x10b, 2);
    bytes[0x3c..0x40].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(
        parse_pe_header_prefix(&bytes),
        Err(PeHeaderError::OutOfBounds {
            offset: FileOffset::new(u64::from(u32::MAX)),
            needed: 4,
            available: 0,
        })
    );
}

#[test]
fn does_not_impose_dos_stub_length_or_alignment_policy() {
    for offset in [2, 3, 0x81] {
        assert_eq!(
            parse_pe_header_prefix(&fixture(offset, 0x14c, 0x10b, 2))
                .unwrap()
                .pe_offset,
            FileOffset::new(offset as u64)
        );
    }
}

#[test]
fn reports_exact_required_ranges() {
    let bytes = fixture(0x80, 0x14c, 0x10b, 224);
    for (length, offset, needed, available) in [
        (1, 0, 2, 1),
        (0x3e, 0x3c, 4, 2),
        (0x82, 0x80, 4, 2),
        (0x90, 0x84, 20, 12),
        (0x9a, 0x98, 224, 2),
    ] {
        assert_eq!(
            parse_pe_header_prefix(&bytes[..length]),
            Err(PeHeaderError::OutOfBounds {
                offset: FileOffset::new(offset),
                needed,
                available,
            })
        );
    }
}

#[test]
fn rejects_bad_signatures_at_their_file_offsets() {
    let original = fixture(0x80, 0x14c, 0x10b, 2);
    let mut bytes = original.clone();
    bytes[0] = b'N';
    assert_eq!(
        parse_pe_header_prefix(&bytes),
        Err(PeHeaderError::InvalidDosSignature {
            offset: FileOffset::new(0)
        })
    );
    for index in 0x80..0x84 {
        bytes.clone_from(&original);
        bytes[index] = 0xff;
        assert_eq!(
            parse_pe_header_prefix(&bytes),
            Err(PeHeaderError::InvalidPeSignature {
                offset: FileOffset::new(0x80)
            })
        );
    }
}

#[test]
fn declared_sizes_zero_and_one_do_not_read_trailing_magic() {
    for size in [0, 1] {
        let mut bytes = fixture(0x80, 0x14c, 0x10b, size);
        bytes.resize(0x100, 0);
        bytes[0x98..0x9a].copy_from_slice(&0x10b_u16.to_le_bytes());
        let expected = Err(PeHeaderError::OptionalHeaderTooShort {
            offset: FileOffset::new(0x98),
            declared_size: size,
        });
        assert_eq!(parse_pe_header_prefix(&bytes), expected);
        assert_eq!(parse_pe_header_prefix(&bytes[..0x98]), expected);
    }
}

#[test]
fn unsupported_magic_and_known_machine_mismatches_are_explicit() {
    for magic in [0, 0x107, 0xb01, u16::MAX] {
        assert_eq!(
            parse_pe_header_prefix(&fixture(0x80, 0x14c, magic, 2)),
            Err(PeHeaderError::UnsupportedOptionalMagic {
                offset: FileOffset::new(0x98),
                magic
            })
        );
    }
    for (machine, magic, kind) in [
        (0x14c, 0x20b, PeKind::Pe32Plus),
        (0x8664, 0x10b, PeKind::Pe32),
    ] {
        assert_eq!(
            parse_pe_header_prefix(&fixture(0x80, machine, magic, 2)),
            Err(PeHeaderError::MachineKindMismatch {
                offset: FileOffset::new(0x84),
                machine,
                kind
            })
        );
    }
}

#[test]
fn full_maximum_declared_optional_range_is_required() {
    let bytes = fixture(0x80, 0, 0x20b, u16::MAX);
    assert_eq!(
        parse_pe_header_prefix(&bytes)
            .unwrap()
            .size_of_optional_header,
        u16::MAX
    );
    assert_eq!(
        parse_pe_header_prefix(&bytes[..bytes.len() - 1]),
        Err(PeHeaderError::OutOfBounds {
            offset: FileOffset::new(0x98),
            needed: u64::from(u16::MAX),
            available: u64::from(u16::MAX) - 1,
        })
    );
}

#[test]
#[ignore = "requires an explicitly built R3-7 fixture in RING3_PE32_FIXTURE"]
fn generated_corpus_matches_recorded_header_metadata() {
    let path = std::env::var_os("RING3_PE32_FIXTURE")
        .expect("RING3_PE32_FIXTURE must name the generated R3-7 pe32-arithmetic.exe");
    let bytes = std::fs::read(path).expect("generated fixture must be readable");
    assert_eq!(bytes.len(), 1024);
    assert_eq!(
        parse_pe_header_prefix(&bytes),
        Ok(PeHeaderPrefix {
            pe_offset: FileOffset::new(0x78),
            machine: 0x14c,
            number_of_sections: 1,
            characteristics: 0x103,
            size_of_optional_header: 224,
            kind: PeKind::Pe32,
        })
    );
}
