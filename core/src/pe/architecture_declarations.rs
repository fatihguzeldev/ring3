use super::{
    PeClrError, PeClrHeaderEvidence, PeDeclaredEvidence, PeHeaderError, PeHeaderPrefixEvidence,
    PeOptionalHeaderEvidence,
};

/// a named mask observation in an available supplied raw word.
/// `is_set == false` means the mask is clear, not that evidence was unavailable.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeFlagBit<T> {
    pub name: &'static str,
    pub mask: T,
    pub is_set: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeCoffArchitectureDeclarations {
    pub raw: PeHeaderPrefixEvidence,
    /// ordered masks: `IMAGE_FILE_32BIT_MACHINE`, `IMAGE_FILE_LARGE_ADDRESS_AWARE`.
    pub bits: [PeFlagBit<u16>; 2],
    /// all bits outside the selected masks, including recognized unselected bits.
    pub unselected_bits: u16,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeClrArchitectureDeclarations {
    pub raw: PeClrHeaderEvidence,
    /// ordered masks: `COMIMAGE_FLAGS_ILONLY`, `COMIMAGE_FLAGS_32BITREQUIRED`,
    /// `COMIMAGE_FLAGS_NATIVE_ENTRYPOINT`, `COMIMAGE_FLAGS_32BITPREFERRED`.
    pub bits: [PeFlagBit<u32>; 4],
    /// all bits outside the selected masks, including recognized unselected bits.
    pub unselected_bits: u32,
}

/// owned raw observations with the supplied independent reader outcomes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeArchitectureDeclarations {
    pub prefix: Result<PeCoffArchitectureDeclarations, PeHeaderError>,
    pub optional: Result<PeOptionalHeaderEvidence, PeHeaderError>,
    pub clr: Result<Option<PeClrArchitectureDeclarations>, PeClrError>,
}

/// names selected raw flag bits without reading bytes or validating evidence.
///
/// accepts existing single-image or named-source evidence, including values
/// constructed by callers. raw fields, offsets, widths and independent outcomes
/// pass through unchanged; their validity and provenance are not authenticated.
/// errors and clr absence produce no bit observations. optional evidence is unchanged.
///
/// coff masks are 0x0100 and 0x0020; clr masks are 0x1, 0x2, 0x10 and 0x20000,
/// in the order documented on each result. set/clear describes only those bits.
/// the required/preferred clr pair is not normalized into a requirement boolean.
/// machine, kind and entry words remain uninterpreted. no cpu, runtime, support,
/// native-only, eligibility or confidence verdict is produced.
///
/// # example
/// ```
/// use ring3_core::{describe_pe_architecture_declarations, inspect_pe_declared_evidence};
/// let evidence = inspect_pe_declared_evidence(b"bad");
/// let declarations = describe_pe_architecture_declarations(evidence);
/// assert_eq!(declarations.prefix.map(|value| value.raw), evidence.prefix);
/// assert_eq!(declarations.optional, evidence.optional);
/// assert!(declarations.clr.is_err());
/// ```
#[must_use]
pub fn describe_pe_architecture_declarations(
    evidence: PeDeclaredEvidence,
) -> PeArchitectureDeclarations {
    PeArchitectureDeclarations {
        prefix: evidence.prefix.map(|raw| {
            let definitions = [
                ("IMAGE_FILE_32BIT_MACHINE", 0x0100),
                ("IMAGE_FILE_LARGE_ADDRESS_AWARE", 0x0020),
            ];
            PeCoffArchitectureDeclarations {
                raw,
                bits: definitions.map(|(name, mask)| PeFlagBit {
                    name,
                    mask,
                    is_set: raw.characteristics.value & mask != 0,
                }),
                unselected_bits: raw.characteristics.value & !0x0120,
            }
        }),
        optional: evidence.optional,
        clr: evidence.clr.map(|value| {
            value.map(|raw| {
                let definitions = [
                    ("COMIMAGE_FLAGS_ILONLY", 0x0000_0001),
                    ("COMIMAGE_FLAGS_32BITREQUIRED", 0x0000_0002),
                    ("COMIMAGE_FLAGS_NATIVE_ENTRYPOINT", 0x0000_0010),
                    ("COMIMAGE_FLAGS_32BITPREFERRED", 0x0002_0000),
                ];
                PeClrArchitectureDeclarations {
                    raw,
                    bits: definitions.map(|(name, mask)| PeFlagBit {
                        name,
                        mask,
                        is_set: raw.flags.value & mask != 0,
                    }),
                    unselected_bits: raw.flags.value & !0x0002_0013,
                }
            })
        }),
    }
}
