use crate::{
    PeDelayImportEvidence, PeDelayImportEvidenceError, PeDelayImportEvidenceLimits,
    PeExportEvidence, PeExportEvidenceError, PeExportEvidenceLimits, PeFingerprintError,
    PeFingerprintedEvidence, PeStaticImportEvidence, PeStaticImportEvidenceError,
    PeStaticImportEvidenceLimits, fingerprint_pe_declared_evidence, inspect_pe_delay_imports,
    inspect_pe_exports, inspect_pe_static_imports,
};

/// one family's logical output limits; not allocation or memory limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeModuleOutputLimits {
    pub max_rows: u64,
    pub max_text_bytes: u64,
}

/// common input admission and separate static, delay and export output limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeModuleEvidenceLimits {
    pub max_input_bytes: u64,
    pub static_imports: PeModuleOutputLimits,
    pub delay_imports: PeModuleOutputLimits,
    pub exports: PeModuleOutputLimits,
}

/// owned fingerprinted declarations and import/export observations, not a complete module report.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeModuleEvidence {
    pub fingerprinted: PeFingerprintedEvidence,
    pub static_imports: Result<PeStaticImportEvidence, PeStaticImportEvidenceError>,
    pub delay_imports: Result<PeDelayImportEvidence, PeDelayImportEvidenceError>,
    pub exports: Result<PeExportEvidence, PeExportEvidenceError>,
}

/// collects fingerprinted declarations and three metadata families from the same bytes.
/// whole-input fingerprint admission runs first: caller byte limit, then sha-256
/// message-length limit. admitted malformed inputs still retain their digest and
/// independent reader errors. all three family results survive independently;
/// a family's output refusal does not discard the fingerprint or other results.
/// each family's existing input-error variant remains in its result type but
/// cannot arise here after common input admission succeeds.
///
/// each family applies its existing row-before-text limits; there is no combined
/// output limit or all-or-nothing output admission. prior owned families can exist
/// before a later refusal. fingerprint/declaration work and existing reader
/// allocations occur earlier; readers may parse the same bytes again. these
/// logical output limits do not cap process memory or recover allocation failure.
///
/// all results outlive input. coordinates, raw text, errors and absence/empty
/// distinctions are preserved without normalization or inference. this does not
/// resolve providers, infer requirements, walk forwarders or establish loadability.
/// the digest covers every byte; digest equality and caller-constructible evidence
/// do not authenticate content or define a cache identity policy.
///
/// ```
/// use ring3_core::{PeModuleEvidenceLimits, PeModuleOutputLimits, inspect_pe_module_evidence};
///
/// let zero = PeModuleOutputLimits { max_rows: 0, max_text_bytes: 0 };
/// let evidence = {
///     let bytes = Vec::new();
///     inspect_pe_module_evidence(&bytes, PeModuleEvidenceLimits {
///         max_input_bytes: 0, static_imports: zero, delay_imports: zero, exports: zero,
///     }).unwrap()
/// };
/// assert_eq!(evidence.fingerprinted.byte_length, 0);
/// assert!(evidence.static_imports.unwrap().descriptors.is_err());
/// assert!(evidence.exports.unwrap().directory.is_err());
/// ```
///
/// # errors
/// returns the fingerprint owner's input or sha-256 length refusal unchanged.
/// family output refusals stay nested within a successful module observation.
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn inspect_pe_module_evidence(
    bytes: &[u8],
    limits: PeModuleEvidenceLimits,
) -> Result<PeModuleEvidence, PeFingerprintError> {
    let fingerprinted = fingerprint_pe_declared_evidence(bytes, limits.max_input_bytes)?;
    let static_imports = inspect_pe_static_imports(
        bytes,
        PeStaticImportEvidenceLimits {
            max_input_bytes: limits.max_input_bytes,
            max_output_rows: limits.static_imports.max_rows,
            max_output_text_bytes: limits.static_imports.max_text_bytes,
        },
    );
    let delay_imports = inspect_pe_delay_imports(
        bytes,
        PeDelayImportEvidenceLimits {
            max_input_bytes: limits.max_input_bytes,
            max_output_rows: limits.delay_imports.max_rows,
            max_output_text_bytes: limits.delay_imports.max_text_bytes,
        },
    );
    let exports = inspect_pe_exports(
        bytes,
        PeExportEvidenceLimits {
            max_input_bytes: limits.max_input_bytes,
            max_output_rows: limits.exports.max_rows,
            max_output_text_bytes: limits.exports.max_text_bytes,
        },
    );
    Ok(PeModuleEvidence {
        fingerprinted,
        static_imports,
        delay_imports,
        exports,
    })
}
