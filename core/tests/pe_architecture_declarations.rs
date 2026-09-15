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
