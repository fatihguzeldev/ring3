use ring3_core::{
    FileOffset, PeClrDescriptorEvidence, PeClrError, PeClrHeaderEvidence, PeDeclaredEvidence,
    PeDesktopExecutableCandidateAssessment as Assessment,
    PeDesktopExecutableCandidateDecision as Decision, PeDesktopExecutableCandidateReason as Reason,
    PeFieldEvidence, PeHeaderError, PeHeaderPrefixEvidence, PeKind, PeOptionalHeaderEvidence,
    RelativeVirtualAddress as Rva, assess_pe_desktop_executable_candidate,
    inspect_pe_declared_evidence,
};

fn field<T>(value: T, offset: u64, byte_length: u8) -> PeFieldEvidence<T> {
    PeFieldEvidence {
        value,
        file_offset: FileOffset::new(offset),
        byte_length,
    }
}

fn supplied(kind: PeKind, flags: u16, subsystem: u16) -> PeDeclaredEvidence {
    PeDeclaredEvidence {
        prefix: Ok(PeHeaderPrefixEvidence {
            kind: field(kind, 152, 2),
            machine: field(0x14c, 132, 2),
            characteristics: field(flags, 150, 2),
        }),
        optional: Ok(PeOptionalHeaderEvidence {
            entry_rva: field(Rva::new(4096), 168, 4),
            subsystem: field(subsystem, 220, 2),
            dll_characteristics: field(0, 222, 2),
            directory_count: field(0, 244, 4),
            clr_descriptor: None,
        }),
        clr: Ok(None),
    }
}

fn check(raw: PeDeclaredEvidence, decision: Decision, reasons: [Option<Reason>; 4]) {
    let original = raw;
    let result = assess_pe_desktop_executable_candidate(raw);
    assert_eq!(
        result,
        Assessment {
            raw: original,
            decision,
            reasons
        }
    );
    assert_eq!(assess_pe_desktop_executable_candidate(raw), result);
    assert_eq!(raw, original);
}

#[test]
fn retains_all_role_reasons_in_order_for_both_widths_and_unselected_backgrounds() {
    use Reason::{DllBitSet as Dll, ExecutableImageBitClear as Clear, SystemBitSet as System};
    for (flags, decision, reasons) in [
        (0x0000, Decision::Excluded, [Some(Clear), None, None, None]),
        (0x0002, Decision::Candidate, [None; 4]),
        (
            0x1000,
            Decision::Excluded,
            [Some(Clear), Some(System), None, None],
        ),
        (0x1002, Decision::Excluded, [Some(System), None, None, None]),
        (
            0x2000,
            Decision::Excluded,
            [Some(Clear), Some(Dll), None, None],
        ),
        (0x2002, Decision::Excluded, [Some(Dll), None, None, None]),
        (
            0x3000,
            Decision::Excluded,
            [Some(Clear), Some(Dll), Some(System), None],
        ),
        (
            0x3002,
            Decision::Excluded,
            [Some(Dll), Some(System), None, None],
        ),
    ] {
        for kind in [PeKind::Pe32, PeKind::Pe32Plus] {
            for subsystem in [2, 3] {
                for background in [0, 0xcffd] {
                    check(
                        supplied(kind, flags + background, subsystem),
                        decision,
                        reasons,
                    );
                }
            }
        }
    }
}

#[test]
fn other_subsystems_are_outside_this_policy_and_keep_all_role_exclusions() {
    for subsystem in [0, 1, 5, 7, 8, 9, 10, 16, u16::MAX] {
        check(
            supplied(PeKind::Pe32, 2, subsystem),
            Decision::Excluded,
            [Some(Reason::SubsystemOutsidePolicy), None, None, None],
        );
        check(
            supplied(PeKind::Pe32Plus, 0x3000, subsystem),
            Decision::Excluded,
            [
                Some(Reason::ExecutableImageBitClear),
                Some(Reason::DllBitSet),
                Some(Reason::SystemBitSet),
                Some(Reason::SubsystemOutsidePolicy),
            ],
        );
    }
}

#[test]
fn unavailable_stages_are_independent_and_keep_their_exact_errors() {
    let prefix_error = PeHeaderError::InvalidPeSignature {
        offset: FileOffset::new(17),
    };
    let optional_error = PeHeaderError::OutOfBounds {
        offset: FileOffset::new(u64::MAX),
        needed: u64::MAX,
        available: 3,
    };
    let mut raw = supplied(PeKind::Pe32, 2, 2);
    raw.prefix = Err(prefix_error);
    check(
        raw,
        Decision::Indeterminate,
        [Some(Reason::PrefixUnavailable), None, None, None],
    );
    raw.optional = Err(optional_error);
    check(
        raw,
        Decision::Indeterminate,
        [
            Some(Reason::PrefixUnavailable),
            Some(Reason::OptionalHeaderUnavailable),
            None,
            None,
        ],
    );
    raw.prefix = supplied(PeKind::Pe32, 2, 2).prefix;
    check(
        raw,
        Decision::Indeterminate,
        [Some(Reason::OptionalHeaderUnavailable), None, None, None],
    );
}

#[test]
fn every_available_exclusion_wins_without_hiding_unavailable_reasons() {
    let error = PeHeaderError::UnsupportedOptionalMagic {
        offset: FileOffset::new(u64::MAX),
        magic: u16::MAX,
    };
    let mut raw = supplied(PeKind::Pe32, 2, 1);
    raw.prefix = Err(error);
    check(
        raw,
        Decision::Excluded,
        [
            Some(Reason::PrefixUnavailable),
            Some(Reason::SubsystemOutsidePolicy),
            None,
            None,
        ],
    );
    for (flags, reasons) in [
        (
            0,
            [
                Some(Reason::ExecutableImageBitClear),
                Some(Reason::OptionalHeaderUnavailable),
                None,
                None,
            ],
        ),
        (
            0x2002,
            [
                Some(Reason::DllBitSet),
                Some(Reason::OptionalHeaderUnavailable),
                None,
                None,
            ],
        ),
        (
            0x1002,
            [
                Some(Reason::SystemBitSet),
                Some(Reason::OptionalHeaderUnavailable),
                None,
                None,
            ],
        ),
        (
            0x3000,
            [
                Some(Reason::ExecutableImageBitClear),
                Some(Reason::DllBitSet),
                Some(Reason::SystemBitSet),
                Some(Reason::OptionalHeaderUnavailable),
            ],
        ),
    ] {
        let mut raw = supplied(PeKind::Pe32Plus, flags, 2);
        raw.optional = Err(error);
        check(raw, Decision::Excluded, reasons);
    }
}

#[test]
fn other_fields_and_forged_coordinates_cannot_promote_or_reject_a_candidate() {
    for (flags, decision, reasons) in [
        (2, Decision::Candidate, [None; 4]),
        (
            0,
            Decision::Excluded,
            [Some(Reason::ExecutableImageBitClear), None, None, None],
        ),
    ] {
        let raw = supplied(PeKind::Pe32Plus, flags, 3);
        let mut variants = [raw; 6];
        variants[0].prefix.as_mut().unwrap().machine = field(u16::MAX, u64::MAX, 255);
        variants[1].optional.as_mut().unwrap().entry_rva = field(Rva::new(0), u64::MAX, 0);
        variants[2].clr = Err(PeClrError::UnsupportedHeaderSize {
            header_size: 71,
            required: 72,
        });
        variants[3].clr = Ok(Some(PeClrHeaderEvidence {
            flags: field(u32::MAX, 0, 255),
            raw_entry_point: field(u32::MAX, u64::MAX, 0),
        }));
        let optional = variants[4].optional.as_mut().unwrap();
        optional.dll_characteristics = field(u16::MAX, 0, 0);
        optional.directory_count = field(u32::MAX, u64::MAX, 255);
        optional.clr_descriptor = Some(PeClrDescriptorEvidence {
            rva: field(Rva::new(u32::MAX), u64::MAX, 0),
            size: field(u32::MAX, 0, 255),
        });
        let prefix = variants[5].prefix.as_mut().unwrap();
        prefix.kind = field(PeKind::Pe32, u64::MAX, 0);
        prefix.characteristics.file_offset = FileOffset::new(u64::MAX);
        prefix.characteristics.byte_length = 255;
        variants[5].optional.as_mut().unwrap().subsystem.file_offset = FileOffset::new(u64::MAX);
        variants[5].optional.as_mut().unwrap().subsystem.byte_length = 0;
        for variant in variants {
            check(variant, decision, reasons);
        }
    }
}

#[test]
fn retained_reader_errors_and_assessment_are_owned_copy_values() {
    fn assert_copy<T: Copy>(_: T) {}
    let raw = {
        let bytes = b"bad".to_vec();
        inspect_pe_declared_evidence(&bytes)
    };
    assert!(raw.prefix.is_err() && raw.optional.is_err() && raw.clr.is_err());
    let result = assess_pe_desktop_executable_candidate(raw);
    assert_copy(result);
    assert_eq!(
        result,
        Assessment {
            raw,
            decision: Decision::Indeterminate,
            reasons: [
                Some(Reason::PrefixUnavailable),
                Some(Reason::OptionalHeaderUnavailable),
                None,
                None,
            ]
        }
    );
}
