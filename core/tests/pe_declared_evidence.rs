use ring3_core::{
    FileOffset, PeClrDescriptorEvidence, PeClrError, PeClrHeaderEvidence, PeDeclaredEvidence,
    PeFieldEvidence, PeHeaderError, PeHeaderPrefixEvidence, PeKind, PeOptionalHeaderEvidence,
    PeRvaError, RelativeVirtualAddress, inspect_pe_declared_evidence,
};

fn put16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn fixed(kind: PeKind) -> usize {
    match kind {
        PeKind::Pe32 => 96,
        PeKind::Pe32Plus => 112,
    }
}

fn fixture(kind: PeKind, pe: usize, raw: usize) -> Vec<u8> {
    let mut bytes = vec![0; raw + 1024];
    let optional = pe + 24;
    let section = optional + fixed(kind) + 128;
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 0x3c, u32::try_from(pe).unwrap());
    bytes[pe..pe + 4].copy_from_slice(b"PE\0\0");
    put16(&mut bytes, pe + 4, 0xffff);
    put16(&mut bytes, pe + 6, 1);
    put16(
        &mut bytes,
        pe + 20,
        u16::try_from(fixed(kind) + 128).unwrap(),
    );
    put16(&mut bytes, pe + 22, 0x1234);
    put16(
        &mut bytes,
        optional,
        if kind == PeKind::Pe32 { 0x10b } else { 0x20b },
    );
    put32(&mut bytes, optional + 16, 0x8765_4321);
    put32(&mut bytes, optional + 60, u32::try_from(raw).unwrap());
    put16(&mut bytes, optional + 68, 0xffff);
    put16(&mut bytes, optional + 70, 0xa140);
    put32(&mut bytes, optional + fixed(kind) - 4, 16);
    put32(&mut bytes, optional + fixed(kind) + 112, 4184);
    put32(&mut bytes, optional + fixed(kind) + 116, 72);
    put32(&mut bytes, section + 8, 1024);
    put32(&mut bytes, section + 12, 4096);
    put32(&mut bytes, section + 16, 1024);
    put32(&mut bytes, section + 20, u32::try_from(raw).unwrap());
    put32(&mut bytes, raw + 88, 72);
    put32(&mut bytes, raw + 104, 0x8123_4511);
    put32(&mut bytes, raw + 108, 0x1234_5678);
    bytes
}

fn field<T>(value: T, offset: usize, byte_length: u8) -> PeFieldEvidence<T> {
    PeFieldEvidence {
        value,
        file_offset: FileOffset::new(offset as u64),
        byte_length,
    }
}

fn expected(kind: PeKind, pe: usize, raw: usize) -> PeDeclaredEvidence {
    let optional = pe + 24;
    PeDeclaredEvidence {
        prefix: Ok(PeHeaderPrefixEvidence {
            kind: field(kind, optional, 2),
            machine: field(0xffff, pe + 4, 2),
            characteristics: field(0x1234, pe + 22, 2),
        }),
        optional: Ok(PeOptionalHeaderEvidence {
            entry_rva: field(RelativeVirtualAddress::new(0x8765_4321), optional + 16, 4),
            subsystem: field(0xffff, optional + 68, 2),
            dll_characteristics: field(0xa140, optional + 70, 2),
            directory_count: field(16, optional + fixed(kind) - 4, 4),
            clr_descriptor: Some(PeClrDescriptorEvidence {
                rva: field(
                    RelativeVirtualAddress::new(4184),
                    optional + fixed(kind) + 112,
                    4,
                ),
                size: field(72, optional + fixed(kind) + 116, 4),
            }),
        }),
        clr: Ok(Some(PeClrHeaderEvidence {
            flags: field(0x8123_4511, raw + 104, 4),
            raw_entry_point: field(0x1234_5678, raw + 108, 4),
        })),
    }
}

#[test]
fn preserves_raw_values_and_physical_positions_in_both_layouts() {
    for kind in [PeKind::Pe32, PeKind::Pe32Plus] {
        for (pe, raw) in [(64, 513), (128, 512), (256, 1024)] {
            assert_eq!(
                inspect_pe_declared_evidence(&fixture(kind, pe, raw)),
                expected(kind, pe, raw)
            );
        }
    }
}

#[test]
fn prefix_survives_a_full_optional_header_error() {
    for kind in [PeKind::Pe32, PeKind::Pe32Plus] {
        let mut bytes = fixture(kind, 128, 512);
        put16(&mut bytes, 148, 2);
        let error = PeHeaderError::OptionalHeaderExtentTooShort {
            offset: FileOffset::new(152),
            required: fixed(kind) as u64,
            declared: 2,
        };
        assert_eq!(
            inspect_pe_declared_evidence(&bytes),
            PeDeclaredEvidence {
                prefix: expected(kind, 128, 512).prefix,
                optional: Err(error),
                clr: Err(PeClrError::Base(PeRvaError::Parse(error))),
            }
        );
    }
}

#[test]
fn optional_evidence_survives_a_malformed_clr_header() {
    for kind in [PeKind::Pe32, PeKind::Pe32Plus] {
        let mut bytes = fixture(kind, 128, 512);
        put32(&mut bytes, 600, 71);
        let mut want = expected(kind, 128, 512);
        want.clr = Err(PeClrError::UnsupportedHeaderSize {
            header_size: 71,
            required: 72,
        });
        assert_eq!(inspect_pe_declared_evidence(&bytes), want);
    }
}

#[test]
fn distinguishes_undeclared_zero_and_inconsistent_descriptors() {
    for kind in [PeKind::Pe32, PeKind::Pe32Plus] {
        for (count, rva, size) in [(14, 4184, 72), (15, 0, 0), (15, 0, 72), (15, 4184, 0)] {
            let mut bytes = fixture(kind, 128, 512);
            let count_offset = 152 + fixed(kind) - 4;
            let descriptor = 152 + fixed(kind) + 112;
            put32(&mut bytes, count_offset, count);
            put32(&mut bytes, descriptor, rva);
            put32(&mut bytes, descriptor + 4, size);
            let mut want = expected(kind, 128, 512);
            let optional = want.optional.as_mut().unwrap();
            optional.directory_count.value = count;
            optional.clr_descriptor = (count > 14).then_some(PeClrDescriptorEvidence {
                rva: field(RelativeVirtualAddress::new(rva), descriptor, 4),
                size: field(size, descriptor + 4, 4),
            });
            want.clr = if count == 14 || (rva == 0 && size == 0) {
                Ok(None)
            } else {
                Err(PeClrError::InconsistentDirectory {
                    rva: RelativeVirtualAddress::new(rva),
                    size,
                })
            };
            assert_eq!(inspect_pe_declared_evidence(&bytes), want);
        }
    }
}

#[test]
fn absent_clr_requires_valid_sections_without_erasing_valid_headers() {
    for kind in [PeKind::Pe32, PeKind::Pe32Plus] {
        let mut bytes = fixture(kind, 128, 512);
        put32(&mut bytes, 152 + fixed(kind) - 4, 14);
        put16(&mut bytes, 134, 97);
        let mut want = expected(kind, 128, 512);
        let optional = want.optional.as_mut().unwrap();
        optional.directory_count.value = 14;
        optional.clr_descriptor = None;
        want.clr = Err(PeClrError::Base(PeRvaError::Parse(
            PeHeaderError::SectionLimitExceeded {
                offset: FileOffset::new(134),
                count: 97,
                limit: 96,
            },
        )));
        assert_eq!(inspect_pe_declared_evidence(&bytes), want);
    }
}

#[test]
fn preserves_prefix_and_exact_directory_limit_error_operands() {
    for kind in [PeKind::Pe32, PeKind::Pe32Plus] {
        let mut bytes = fixture(kind, 128, 512);
        put16(&mut bytes, 148, u16::try_from(fixed(kind) + 136).unwrap());
        put32(&mut bytes, 152 + fixed(kind) - 4, 17);
        let error = PeHeaderError::DirectoryLimitExceeded {
            offset: FileOffset::new((152 + fixed(kind) - 4) as u64),
            count: 17,
            limit: 16,
        };
        assert_eq!(
            inspect_pe_declared_evidence(&bytes),
            PeDeclaredEvidence {
                prefix: expected(kind, 128, 512).prefix,
                optional: Err(error),
                clr: Err(PeClrError::Base(PeRvaError::Parse(error))),
            }
        );
    }
}

#[test]
fn retains_independent_typed_outcomes_for_an_unreadable_prefix() {
    let error = PeHeaderError::OutOfBounds {
        offset: FileOffset::new(0),
        needed: 2,
        available: 0,
    };
    assert_eq!(
        inspect_pe_declared_evidence(&[]),
        PeDeclaredEvidence {
            prefix: Err(error),
            optional: Err(error),
            clr: Err(PeClrError::Base(PeRvaError::Parse(error))),
        }
    );
}

#[test]
fn results_are_owned_and_inspection_preserves_input() {
    let original = fixture(PeKind::Pe32Plus, 128, 513);
    let mut bytes = original.clone();
    let evidence = inspect_pe_declared_evidence(&bytes);
    assert_eq!(bytes, original);
    assert_eq!(inspect_pe_declared_evidence(&bytes), evidence);
    bytes.fill(0);
    drop(bytes);
    assert_eq!(evidence, expected(PeKind::Pe32Plus, 128, 513));
    assert_eq!(inspect_pe_declared_evidence(&original), evidence);
}

fn check_raw_field<T>(bytes: &[u8], evidence: &PeFieldEvidence<T>, raw: &[u8]) {
    assert_eq!(usize::from(evidence.byte_length), raw.len());
    let start = usize::try_from(evidence.file_offset.get()).unwrap();
    let end = start.checked_add(raw.len()).unwrap();
    assert_eq!(bytes.get(start..end), Some(raw));
}

fn generated_declared_evidence(variable: &str, size: usize, wanted: PeDeclaredEvidence) {
    let path = std::env::var(variable).expect("set the generated fixture path");
    let original = std::fs::read(&path).unwrap();
    assert_eq!(original.len(), size);
    let mut bytes = original.clone();
    let actual = inspect_pe_declared_evidence(&bytes);
    assert_eq!(actual, wanted);
    assert_eq!(inspect_pe_declared_evidence(&bytes), actual);
    assert_eq!(bytes, original);
    let prefix = actual.prefix.unwrap();
    let magic: u16 = match prefix.kind.value {
        PeKind::Pe32 => 0x10b,
        PeKind::Pe32Plus => 0x20b,
    };
    check_raw_field(&bytes, &prefix.kind, &magic.to_le_bytes());
    check_raw_field(&bytes, &prefix.machine, &prefix.machine.value.to_le_bytes());
    check_raw_field(
        &bytes,
        &prefix.characteristics,
        &prefix.characteristics.value.to_le_bytes(),
    );
    let optional = actual.optional.unwrap();
    check_raw_field(
        &bytes,
        &optional.entry_rva,
        &optional.entry_rva.value.get().to_le_bytes(),
    );
    check_raw_field(
        &bytes,
        &optional.subsystem,
        &optional.subsystem.value.to_le_bytes(),
    );
    check_raw_field(
        &bytes,
        &optional.dll_characteristics,
        &optional.dll_characteristics.value.to_le_bytes(),
    );
    check_raw_field(
        &bytes,
        &optional.directory_count,
        &optional.directory_count.value.to_le_bytes(),
    );
    let descriptor = optional.clr_descriptor.unwrap();
    check_raw_field(
        &bytes,
        &descriptor.rva,
        &descriptor.rva.value.get().to_le_bytes(),
    );
    check_raw_field(
        &bytes,
        &descriptor.size,
        &descriptor.size.value.to_le_bytes(),
    );
    if let Some(clr) = actual.clr.unwrap() {
        check_raw_field(&bytes, &clr.flags, &clr.flags.value.to_le_bytes());
        check_raw_field(
            &bytes,
            &clr.raw_entry_point,
            &clr.raw_entry_point.value.to_le_bytes(),
        );
    }
    bytes.fill(0);
    drop(bytes);
    assert_eq!(actual, wanted);
    assert_eq!(inspect_pe_declared_evidence(&original), actual);
    assert_eq!(std::fs::read(path).unwrap(), original);
}

#[test]
#[ignore = "requires a generated unpatched pe32 fixture"]
fn generated_pe32_declared_evidence_matches_compiled_fields() {
    generated_declared_evidence(
        "RING3_PE32_FIXTURE",
        1024,
        PeDeclaredEvidence {
            prefix: Ok(PeHeaderPrefixEvidence {
                kind: field(PeKind::Pe32, 144, 2),
                machine: field(332, 124, 2),
                characteristics: field(259, 142, 2),
            }),
            optional: Ok(PeOptionalHeaderEvidence {
                entry_rva: field(RelativeVirtualAddress::new(4096), 160, 4),
                subsystem: field(3, 212, 2),
                dll_characteristics: field(33024, 214, 2),
                directory_count: field(16, 236, 4),
                clr_descriptor: Some(PeClrDescriptorEvidence {
                    rva: field(RelativeVirtualAddress::new(0), 352, 4),
                    size: field(0, 356, 4),
                }),
            }),
            clr: Ok(None),
        },
    );
}

#[test]
#[ignore = "requires a generated unpatched pe32+ fixture"]
fn generated_pe32plus_declared_evidence_matches_compiled_fields() {
    generated_declared_evidence(
        "RING3_PE32PLUS_FIXTURE",
        1024,
        PeDeclaredEvidence {
            prefix: Ok(PeHeaderPrefixEvidence {
                kind: field(PeKind::Pe32Plus, 144, 2),
                machine: field(34404, 124, 2),
                characteristics: field(35, 142, 2),
            }),
            optional: Ok(PeOptionalHeaderEvidence {
                entry_rva: field(RelativeVirtualAddress::new(4096), 160, 4),
                subsystem: field(3, 212, 2),
                dll_characteristics: field(33056, 214, 2),
                directory_count: field(16, 252, 4),
                clr_descriptor: Some(PeClrDescriptorEvidence {
                    rva: field(RelativeVirtualAddress::new(0), 368, 4),
                    size: field(0, 372, 4),
                }),
            }),
            clr: Ok(None),
        },
    );
}

#[test]
#[ignore = "requires a generated unpatched managed pe32 fixture"]
fn generated_managed_pe32_declared_evidence_matches_compiled_fields() {
    generated_declared_evidence(
        "RING3_CLR_PE32_FIXTURE",
        3584,
        PeDeclaredEvidence {
            prefix: Ok(PeHeaderPrefixEvidence {
                kind: field(PeKind::Pe32, 152, 2),
                machine: field(332, 132, 2),
                characteristics: field(258, 150, 2),
            }),
            optional: Ok(PeOptionalHeaderEvidence {
                entry_rva: field(RelativeVirtualAddress::new(9078), 168, 4),
                subsystem: field(3, 220, 2),
                dll_characteristics: field(34112, 222, 2),
                directory_count: field(16, 244, 4),
                clr_descriptor: Some(PeClrDescriptorEvidence {
                    rva: field(RelativeVirtualAddress::new(8200), 360, 4),
                    size: field(72, 364, 4),
                }),
            }),
            clr: Ok(Some(PeClrHeaderEvidence {
                flags: field(3, 536, 4),
                raw_entry_point: field(0x0600_0001, 540, 4),
            })),
        },
    );
}

#[test]
#[ignore = "requires a generated unpatched managed pe32+ fixture"]
fn generated_managed_pe32plus_declared_evidence_matches_compiled_fields() {
    generated_declared_evidence(
        "RING3_CLR_PE32PLUS_FIXTURE",
        3072,
        PeDeclaredEvidence {
            prefix: Ok(PeHeaderPrefixEvidence {
                kind: field(PeKind::Pe32Plus, 152, 2),
                machine: field(34404, 132, 2),
                characteristics: field(34, 150, 2),
            }),
            optional: Ok(PeOptionalHeaderEvidence {
                entry_rva: field(RelativeVirtualAddress::new(0), 168, 4),
                subsystem: field(3, 220, 2),
                dll_characteristics: field(34112, 222, 2),
                directory_count: field(16, 260, 4),
                clr_descriptor: Some(PeClrDescriptorEvidence {
                    rva: field(RelativeVirtualAddress::new(8192), 376, 4),
                    size: field(72, 380, 4),
                }),
            }),
            clr: Ok(Some(PeClrHeaderEvidence {
                flags: field(1, 528, 4),
                raw_entry_point: field(0x0600_0001, 532, 4),
            })),
        },
    );
}
