use super::{PeFlagBit, PeHeaderPrefixEvidence};

/// independent image-role declarations in one supplied successful prefix.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeCoffImageRoleDeclarations {
    pub raw: PeHeaderPrefixEvidence,
    /// ordered masks: `IMAGE_FILE_EXECUTABLE_IMAGE`, `IMAGE_FILE_SYSTEM`, `IMAGE_FILE_DLL`.
    pub bits: [PeFlagBit<u16>; 3],
    /// all bits outside the selected masks, including recognized unselected bits.
    pub unselected_bits: u16,
}

/// names three coff image-role bits without reading bytes or validating evidence.
///
/// raw fields, offsets and widths pass through unchanged, including caller-created
/// values. masks 0x0002, 0x1000 and 0x2000 are independent; no combination wins or
/// conflicts. clear means available-clear. unselected bits retain everything else.
/// no executable-candidate, driver, service, support or loadability verdict is made.
///
/// accepts only a successful prefix. callers can map a reader result to preserve
/// its error without producing declarations. this fixed-size transformation has
/// no allocation or host access and does not authenticate the supplied evidence.
///
/// # example
/// ```
/// use ring3_core::{describe_pe_coff_image_role_declarations, inspect_pe_declared_evidence};
/// let prefix = inspect_pe_declared_evidence(b"bad").prefix;
/// let declarations = prefix.map(describe_pe_coff_image_role_declarations);
/// assert!(declarations.is_err());
/// assert_eq!(declarations.map(|value| value.raw), prefix);
/// ```
#[must_use]
pub fn describe_pe_coff_image_role_declarations(
    raw: PeHeaderPrefixEvidence,
) -> PeCoffImageRoleDeclarations {
    let definitions = [
        ("IMAGE_FILE_EXECUTABLE_IMAGE", 0x0002),
        ("IMAGE_FILE_SYSTEM", 0x1000),
        ("IMAGE_FILE_DLL", 0x2000),
    ];
    PeCoffImageRoleDeclarations {
        raw,
        bits: definitions.map(|(name, mask)| PeFlagBit {
            name,
            mask,
            is_set: raw.characteristics.value & mask != 0,
        }),
        unselected_bits: raw.characteristics.value & !0x3002,
    }
}
