use super::PeClrHeaderEvidence;
use crate::RelativeVirtualAddress;

/// the declared interpretation of the raw word, without validating its target.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeClrEntryPointTarget {
    ManagedToken(u32),
    NativeRva(RelativeVirtualAddress),
}

/// an owned declaration retaining every supplied raw value and coordinate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeClrEntryPointDeclaration {
    pub raw: PeClrHeaderEvidence,
    pub target: PeClrEntryPointTarget,
}

/// interprets one supplied clr entry word using only the native-entrypoint bit.
///
/// flag 0x10 set selects a native rva; clear selects a managed token. the complete
/// word, including zero and its high bit, is preserved. zero is not converted to
/// absence. no token table, method, target range or executable entry is validated.
/// raw flags, offsets and widths remain unchanged, including unknown flags and
/// inconsistent caller-created evidence. supplied or constructed declarations are
/// not authenticated; this does not select an entry point or imply runtime support.
///
/// accepts only available clr evidence. callers can map their reader `Result`
/// and `Option` to preserve errors and absence without producing a declaration.
/// this fixed-size copy transformation allocates nothing and accesses no host.
///
/// # example
/// ```
/// use ring3_core::{describe_pe_clr_entry_point, inspect_pe_declared_evidence};
/// let clr = inspect_pe_declared_evidence(b"bad").clr;
/// let declarations = clr.map(|value| value.map(describe_pe_clr_entry_point));
/// assert!(declarations.is_err());
/// assert_eq!(declarations.map(|value| value.map(|entry| entry.raw)), clr);
/// ```
#[must_use]
pub fn describe_pe_clr_entry_point(raw: PeClrHeaderEvidence) -> PeClrEntryPointDeclaration {
    let target = if raw.flags.value & 0x10 == 0 {
        PeClrEntryPointTarget::ManagedToken(raw.raw_entry_point.value)
    } else {
        PeClrEntryPointTarget::NativeRva(RelativeVirtualAddress::new(raw.raw_entry_point.value))
    };
    PeClrEntryPointDeclaration { raw, target }
}
