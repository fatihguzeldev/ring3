use ring3_core::{
    AsciiPeSource, AsciiPeSourceEvidenceLimits, AsciiSourcePathLimits, FileOffset,
    PeArchitectureDeclarations, PeClrDescriptorEvidence, PeClrError, PeClrHeaderEvidence,
    PeDeclaredEvidence, PeFieldEvidence, PeFlagBit, PeHeaderBatchLimits, PeHeaderError,
    PeHeaderPrefixEvidence, PeKind, PeOptionalHeaderEvidence, RelativeVirtualAddress,
    describe_pe_architecture_declarations, inspect_ascii_pe_source_evidence,
};

fn field<T>(value: T, offset: u64, byte_length: u8) -> PeFieldEvidence<T> {
    PeFieldEvidence {
        value,
        file_offset: FileOffset::new(offset),
        byte_length,
    }
}

fn supplied(chars: u16, flags: u32) -> PeDeclaredEvidence {
    PeDeclaredEvidence {
        prefix: Ok(PeHeaderPrefixEvidence {
            kind: field(PeKind::Pe32Plus, u64::MAX, 0),
            machine: field(0x14c, 7, 255),
            characteristics: field(chars, 123, 2),
        }),
        optional: Ok(PeOptionalHeaderEvidence {
            entry_rva: field(RelativeVirtualAddress::new(u32::MAX), 9, 4),
            subsystem: field(0xffff, 11, 2),
            dll_characteristics: field(0xffff, 13, 2),
            directory_count: field(u32::MAX, 15, 4),
            clr_descriptor: Some(PeClrDescriptorEvidence {
                rva: field(RelativeVirtualAddress::new(0), 17, 4),
                size: field(72, 21, 4),
            }),
        }),
        clr: Ok(Some(PeClrHeaderEvidence {
            flags: field(flags, 25, 4),
            raw_entry_point: field(u32::MAX, 29, 4),
        })),
    }
}

fn unproject(value: &PeArchitectureDeclarations) -> PeDeclaredEvidence {
    PeDeclaredEvidence {
        prefix: value.prefix.map(|x| x.raw),
        optional: value.optional,
        clr: value.clr.map(|x| x.map(|x| x.raw)),
    }
}

#[test]
fn names_selected_bits_and_preserves_supplied_raw_evidence_without_validation() {
    let evidence = supplied(0x8120, 0x8003_001b);
    let result = describe_pe_architecture_declarations(evidence);
    assert_eq!(unproject(&result), evidence);
    assert_eq!(describe_pe_architecture_declarations(evidence), result);
    let prefix = result.prefix.unwrap();
    assert_eq!(
        prefix.bits,
        [
            PeFlagBit {
                name: "IMAGE_FILE_32BIT_MACHINE",
                mask: 0x0100,
                is_set: true,
            },
            PeFlagBit {
                name: "IMAGE_FILE_LARGE_ADDRESS_AWARE",
                mask: 0x0020,
                is_set: true,
            },
        ]
    );
    assert_eq!(prefix.unselected_bits, 0x8000);
    let clr = result.clr.unwrap().unwrap();
    assert_eq!(
        clr.bits,
        [
            PeFlagBit {
                name: "COMIMAGE_FLAGS_ILONLY",
                mask: 1,
                is_set: true,
            },
            PeFlagBit {
                name: "COMIMAGE_FLAGS_32BITREQUIRED",
                mask: 2,
                is_set: true,
            },
            PeFlagBit {
                name: "COMIMAGE_FLAGS_NATIVE_ENTRYPOINT",
                mask: 0x10,
                is_set: true,
            },
            PeFlagBit {
                name: "COMIMAGE_FLAGS_32BITPREFERRED",
                mask: 0x0002_0000,
                is_set: true,
            },
        ]
    );
    assert_eq!(clr.unselected_bits, 0x8001_0008);
}

#[test]
fn keeps_all_required_preferred_raw_pairs_without_normalizing_them() {
    for (flags, pair) in [
        (0, (false, false)),
        (2, (true, false)),
        (0x0002_0000, (false, true)),
        (0x0002_0002, (true, true)),
    ] {
        let result = describe_pe_architecture_declarations(supplied(0, flags));
        let clr = result.clr.unwrap().unwrap();
        assert_eq!((clr.bits[1].is_set, clr.bits[3].is_set), pair);
        assert_eq!(clr.raw.flags.value, flags);
        assert!(!clr.bits[0].is_set);
        assert!(!clr.bits[2].is_set);
        assert_eq!(clr.unselected_bits, 0);
    }
}

#[test]
fn distinguishes_available_clear_bits_from_absence_and_error() {
    let evidence = supplied(0, 0);
    let result = describe_pe_architecture_declarations(evidence);
    assert!(result.prefix.unwrap().bits.iter().all(|bit| !bit.is_set));
    let clr = result.clr.unwrap().unwrap();
    assert!(clr.bits.iter().all(|bit| !bit.is_set));
    assert_eq!(clr.raw, evidence.clr.unwrap().unwrap());
    let error = PeClrError::UnsupportedHeaderSize {
        header_size: 71,
        required: 72,
    };
    for state in [Ok(None), Err(error)] {
        let supplied = PeDeclaredEvidence {
            clr: state,
            ..evidence
        };
        let result = describe_pe_architecture_declarations(supplied);
        assert_eq!(unproject(&result), supplied);
        assert_eq!(result.clr, state.map(|_| None));
    }
}

#[test]
fn retains_successful_prefix_when_later_supplied_sections_fail() {
    let mut evidence = supplied(u16::MAX, u32::MAX);
    evidence.optional = Err(PeHeaderError::OptionalHeaderExtentTooShort {
        offset: FileOffset::new(88),
        required: 96,
        declared: 2,
    });
    evidence.clr = Err(PeClrError::UnsupportedHeaderSize {
        header_size: 4,
        required: 72,
    });
    let result = describe_pe_architecture_declarations(evidence);
    assert_eq!(unproject(&result), evidence);
    assert_eq!(result.prefix.unwrap().unselected_bits, 0xfedf);
}

#[test]
fn does_not_recompute_independent_supplied_outcomes() {
    let mut evidence = supplied(0, u32::MAX);
    evidence.prefix = Err(PeHeaderError::InvalidDosSignature {
        offset: FileOffset::new(u64::MAX),
    });
    let result = describe_pe_architecture_declarations(evidence);
    assert_eq!(unproject(&result), evidence);
    assert_eq!(result.clr.unwrap().unwrap().unselected_bits, 0xfffd_ffec);
    assert_eq!(result.optional, evidence.optional);
}

#[test]
fn describes_named_source_results_after_source_release() {
    let mut bytes = vec![0; 90];
    bytes[..2].copy_from_slice(b"MZ");
    bytes[60..64].copy_from_slice(&64_u32.to_le_bytes());
    bytes[64..68].copy_from_slice(b"PE\0\0");
    bytes[68..70].copy_from_slice(&0xffff_u16.to_le_bytes());
    bytes[84..86].copy_from_slice(&2_u16.to_le_bytes());
    bytes[86..88].copy_from_slice(&0x0120_u16.to_le_bytes());
    bytes[88..90].copy_from_slice(&0x010b_u16.to_le_bytes());
    let mut path = String::from("Bin/A");
    let batch = inspect_ascii_pe_source_evidence(
        &[AsciiPeSource {
            path: &path,
            bytes: &bytes,
        }],
        AsciiPeSourceEvidenceLimits {
            paths: AsciiSourcePathLimits {
                max_paths: 1,
                max_path_bytes: 5,
                max_total_path_bytes: 5,
                max_depth: 2,
            },
            content: PeHeaderBatchLimits {
                max_files: 1,
                max_file_bytes: 90,
                max_total_bytes: 90,
            },
        },
    )
    .unwrap();
    bytes.fill(0);
    path.clear();
    drop(bytes);
    drop(path);
    let entry = batch.entries.into_iter().next().unwrap();
    let result = describe_pe_architecture_declarations(entry.evidence);
    assert_eq!(entry.path.normalized, "Bin/A");
    assert_eq!(unproject(&result), entry.evidence);
    let prefix = result.prefix.unwrap();
    assert_eq!(prefix.raw.characteristics, field(0x0120, 86, 2));
    assert!(prefix.bits.iter().all(|bit| bit.is_set));
    assert_eq!(prefix.unselected_bits, 0);
    assert!(result.optional.is_err());
    assert!(result.clr.is_err());
}

use ring3_core::{
    PeClrArchitectureDeclarations, PeCoffArchitectureDeclarations, inspect_pe_declared_evidence,
};

fn compiled_expectation(
    raw: PeDeclaredEvidence,
    coff_bits: [bool; 2],
    coff_unselected: u16,
    clr_bits: Option<[bool; 4]>,
) -> PeArchitectureDeclarations {
    assert_eq!(raw.clr.unwrap().is_some(), clr_bits.is_some());
    PeArchitectureDeclarations {
        prefix: raw.prefix.map(|raw| PeCoffArchitectureDeclarations {
            raw,
            bits: [
                PeFlagBit {
                    name: "IMAGE_FILE_32BIT_MACHINE",
                    mask: 0x0100,
                    is_set: coff_bits[0],
                },
                PeFlagBit {
                    name: "IMAGE_FILE_LARGE_ADDRESS_AWARE",
                    mask: 0x0020,
                    is_set: coff_bits[1],
                },
            ],
            unselected_bits: coff_unselected,
        }),
        optional: raw.optional,
        clr: raw.clr.map(|value| {
            value.map(|raw| {
                let states = clr_bits.unwrap();
                PeClrArchitectureDeclarations {
                    raw,
                    bits: [
                        PeFlagBit {
                            name: "COMIMAGE_FLAGS_ILONLY",
                            mask: 1,
                            is_set: states[0],
                        },
                        PeFlagBit {
                            name: "COMIMAGE_FLAGS_32BITREQUIRED",
                            mask: 2,
                            is_set: states[1],
                        },
                        PeFlagBit {
                            name: "COMIMAGE_FLAGS_NATIVE_ENTRYPOINT",
                            mask: 0x10,
                            is_set: states[2],
                        },
                        PeFlagBit {
                            name: "COMIMAGE_FLAGS_32BITPREFERRED",
                            mask: 0x0002_0000,
                            is_set: states[3],
                        },
                    ],
                    unselected_bits: 0,
                }
            })
        }),
    }
}

fn check_raw_field<T>(bytes: &[u8], evidence: &PeFieldEvidence<T>, raw: &[u8]) {
    assert_eq!(usize::from(evidence.byte_length), raw.len());
    let start = usize::try_from(evidence.file_offset.get()).unwrap();
    let end = start.checked_add(raw.len()).unwrap();
    assert_eq!(bytes.get(start..end), Some(raw));
}

fn generated_architecture_declarations(
    variable: &str,
    size: usize,
    wanted: &PeArchitectureDeclarations,
) {
    let path = std::env::var(variable).expect("set the generated fixture path");
    let original = std::fs::read(&path).unwrap();
    assert_eq!(original.len(), size);
    let mut bytes = original.clone();
    let actual = describe_pe_architecture_declarations(inspect_pe_declared_evidence(&bytes));
    assert_eq!(&actual, wanted);
    assert_eq!(
        describe_pe_architecture_declarations(inspect_pe_declared_evidence(&bytes)),
        actual
    );
    assert_eq!(bytes, original);
    let prefix = actual.prefix.unwrap().raw;
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
        let clr = clr.raw;
        check_raw_field(&bytes, &clr.flags, &clr.flags.value.to_le_bytes());
        check_raw_field(
            &bytes,
            &clr.raw_entry_point,
            &clr.raw_entry_point.value.to_le_bytes(),
        );
    }
    bytes.fill(0);
    drop(bytes);
    assert_eq!(&actual, wanted);
    assert_eq!(
        describe_pe_architecture_declarations(inspect_pe_declared_evidence(&original)),
        actual
    );
    assert_eq!(std::fs::read(path).unwrap(), original);
}

#[test]
#[ignore = "requires a generated unpatched pe32 fixture"]
fn generated_pe32_architecture_declarations_match_compiled_fields() {
    generated_architecture_declarations(
        "RING3_PE32_FIXTURE",
        1024,
        &compiled_expectation(
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
            [true, false],
            3,
            None,
        ),
    );
}

#[test]
#[ignore = "requires a generated unpatched pe32+ fixture"]
fn generated_pe32plus_architecture_declarations_match_compiled_fields() {
    generated_architecture_declarations(
        "RING3_PE32PLUS_FIXTURE",
        1024,
        &compiled_expectation(
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
            [false, true],
            3,
            None,
        ),
    );
}

#[test]
#[ignore = "requires a generated unpatched managed pe32 fixture"]
fn generated_managed_pe32_architecture_declarations_match_compiled_fields() {
    generated_architecture_declarations(
        "RING3_CLR_PE32_FIXTURE",
        3584,
        &compiled_expectation(
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
            [true, false],
            2,
            Some([true, true, false, false]),
        ),
    );
}

#[test]
#[ignore = "requires a generated unpatched managed pe32+ fixture"]
fn generated_managed_pe32plus_architecture_declarations_match_compiled_fields() {
    generated_architecture_declarations(
        "RING3_CLR_PE32PLUS_FIXTURE",
        3072,
        &compiled_expectation(
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
            [false, true],
            2,
            Some([true, false, false, false]),
        ),
    );
}
