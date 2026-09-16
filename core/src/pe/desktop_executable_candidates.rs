use super::PeDeclaredEvidence;

/// result of this desktop header policy, not loadability or source authenticity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeDesktopExecutableCandidateDecision {
    Candidate,
    Excluded,
    Indeterminate,
}

/// negative or unavailable policy evidence; original values remain in `raw`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeDesktopExecutableCandidateReason {
    PrefixUnavailable,
    ExecutableImageBitClear,
    DllBitSet,
    SystemBitSet,
    OptionalHeaderUnavailable,
    SubsystemOutsidePolicy,
}

/// reasons occupy a compact prefix in policy order, followed only by `None`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeDesktopExecutableCandidateAssessment {
    pub raw: PeDeclaredEvidence,
    pub decision: PeDesktopExecutableCandidateDecision,
    pub reasons: [Option<PeDesktopExecutableCandidateReason>; 4],
}

/// assesses supplied evidence under a small ring3 desktop header policy.
///
/// a candidate declares executable-image (0x0002), clears dll (0x2000) and system
/// (0x1000), and declares windows gui (2) or cui (3). system and other subsystems
/// are excluded by this policy; exclusion does not mean driver, invalid image or
/// windows incompatibility. the result does not select a main executable.
///
/// reasons are collected independently: prefix unavailable, or executable-clear,
/// dll-set and system-set in that order; then optional unavailable or subsystem
/// outside policy. all present reasons occupy a compact prefix of four slots;
/// the rest are `None`. any available exclusion wins; otherwise missing required
/// evidence yields `Indeterminate`; no reasons yields `Candidate`.
///
/// returns the complete raw value unchanged, including independent errors and
/// caller-constructed inconsistent fields, offsets or widths. kind, machine,
/// entry rva (including zero), dll characteristics, directory count, clr evidence
/// and unselected coff bits do not affect this decision. a candidate may coexist
/// with retained clr errors; this is not a validated image or complete inspection.
///
/// this fixed-size copy transformation allocates nothing and accesses no host.
/// it does not authenticate evidence, rank paths, assign confidence, infer support
/// or establish loadability. caller-constructed assessments are not validated.
///
/// ```
/// use ring3_core::{PeDesktopExecutableCandidateDecision,
///     assess_pe_desktop_executable_candidate, inspect_pe_declared_evidence};
/// let raw = inspect_pe_declared_evidence(b"bad");
/// let assessment = assess_pe_desktop_executable_candidate(raw);
/// assert_eq!(assessment.raw, raw);
/// assert_eq!(assessment.decision, PeDesktopExecutableCandidateDecision::Indeterminate);
/// ```
#[must_use]
pub fn assess_pe_desktop_executable_candidate(
    raw: PeDeclaredEvidence,
) -> PeDesktopExecutableCandidateAssessment {
    use PeDesktopExecutableCandidateDecision as Decision;
    use PeDesktopExecutableCandidateReason as Reason;
    let mut reasons = [None; 4];
    let mut count = 0;
    let mut record = |reason| {
        reasons[count] = Some(reason);
        count += 1;
    };
    match raw.prefix {
        Ok(prefix) => {
            let flags = prefix.characteristics.value;
            if flags & 0x0002 == 0 {
                record(Reason::ExecutableImageBitClear);
            }
            if flags & 0x2000 != 0 {
                record(Reason::DllBitSet);
            }
            if flags & 0x1000 != 0 {
                record(Reason::SystemBitSet);
            }
        }
        Err(_) => record(Reason::PrefixUnavailable),
    }
    match raw.optional {
        Ok(optional) => {
            if !matches!(optional.subsystem.value, 2 | 3) {
                record(Reason::SubsystemOutsidePolicy);
            }
        }
        Err(_) => record(Reason::OptionalHeaderUnavailable),
    }
    let excluded = reasons.iter().flatten().any(|reason| {
        !matches!(
            reason,
            Reason::PrefixUnavailable | Reason::OptionalHeaderUnavailable
        )
    });
    let decision = if excluded {
        Decision::Excluded
    } else if count != 0 {
        Decision::Indeterminate
    } else {
        Decision::Candidate
    };
    PeDesktopExecutableCandidateAssessment {
        raw,
        decision,
        reasons,
    }
}
