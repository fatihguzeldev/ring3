use super::pe_headers::admit_sources;
use super::{
    AsciiPeSource, AsciiPeSourceEvidenceError, AsciiPeSourceEvidenceLimits,
    AsciiPeSourceHeaderError, AsciiPeSourceHeaderLimits, AsciiSourcePathEntry,
};
use crate::{PeFingerprintError, PeFingerprintedEvidence, fingerprint_pe_declared_evidence};

/// owned observations of one caller-paired path and its content.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AsciiPeSourceFingerprint {
    pub path: AsciiSourcePathEntry,
    /// an algorithm-length refusal is local to this entry; later entries continue.
    pub fingerprint: Result<PeFingerprintedEvidence, PeFingerprintError>,
}

/// owned results in input order; public fields are also caller-constructible.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AsciiPeSourceFingerprintBatch {
    pub total_path_bytes: u64,
    /// logical content bytes, counting every alias occurrence and excluding paths.
    pub total_content_bytes: u64,
    pub entries: Vec<AsciiPeSourceFingerprint>,
}

/// admits all paired sources, then fingerprints and inspects each source's bytes.
///
/// shares the header and evidence collectors' admission: path count, complete
/// path admission, then content count, per-file size and checked total. no hash
/// or pe reader runs before that admission completes. zero limits are valid.
///
/// each admitted entry retains the single-input fingerprint result. its sha-256
/// message-length refusal is entry-local; empty or malformed pe bytes otherwise
/// receive a digest and independent reader errors. the same per-file caller cap
/// is forwarded after admission, without introducing another budget.
///
/// aliases are counted and hashed for every occurrence. paths do not participate
/// in digests; indices and lexical keys do not establish external file identity
/// or authenticate the caller's pairing. physical offsets refer to the paired
/// input. path allocation may precede content refusal; logical limits do not
/// bound process memory, elapsed time or acquisition, or perform cancellation.
///
/// # errors
/// returns the shared path or content admission error with its original operands.
/// outer refusals contain no partial batch. fingerprint errors stay in entries.
///
/// # panics
/// only if admitted cardinality or an internal successful-reader invariant fails.
/// malformed input is represented by admission or per-entry reader errors.
///
/// # example
/// ```
/// use ring3_core::{AsciiPeSource, AsciiPeSourceEvidenceLimits, AsciiSourcePathLimits,
///     PeHeaderBatchLimits, fingerprint_ascii_pe_source_evidence};
/// let batch = fingerprint_ascii_pe_source_evidence(
///     &[AsciiPeSource { path: "data", bytes: b"abc" }],
///     AsciiPeSourceEvidenceLimits {
///         paths: AsciiSourcePathLimits {
///             max_paths: 1, max_path_bytes: 4, max_total_path_bytes: 4, max_depth: 1,
///         },
///         content: PeHeaderBatchLimits { max_files: 1, max_file_bytes: 3, max_total_bytes: 3 },
///     },
/// )?;
/// assert_eq!(batch.entries[0].path.key, "data");
/// let content = batch.entries[0].fingerprint.unwrap();
/// assert_eq!(content.byte_length, 3);
/// assert_eq!(&content.digest[..4], &[0xba, 0x78, 0x16, 0xbf]);
/// assert!(content.evidence.prefix.is_err());
/// # Ok::<(), ring3_core::AsciiPeSourceEvidenceError>(())
/// ```
#[expect(
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    reason = "project documentation headings are lower case"
)]
pub fn fingerprint_ascii_pe_source_evidence(
    sources: &[AsciiPeSource<'_>],
    limits: AsciiPeSourceEvidenceLimits,
) -> Result<AsciiPeSourceFingerprintBatch, AsciiPeSourceEvidenceError> {
    let (admitted, total_content_bytes) = admit_sources(
        sources,
        AsciiPeSourceHeaderLimits {
            paths: limits.paths,
            headers: limits.content,
        },
    )
    .map_err(|error| match error {
        AsciiPeSourceHeaderError::Paths(error) => AsciiPeSourceEvidenceError::Paths(error),
        AsciiPeSourceHeaderError::Headers(error) => AsciiPeSourceEvidenceError::Content(error),
    })?;
    assert_eq!(
        admitted.entries.len(),
        sources.len(),
        "admitted source cardinality changed"
    );
    Ok(AsciiPeSourceFingerprintBatch {
        total_path_bytes: admitted.total_path_bytes,
        total_content_bytes,
        entries: admitted
            .entries
            .into_iter()
            .zip(sources)
            .map(|(path, source)| AsciiPeSourceFingerprint {
                path,
                fingerprint: fingerprint_pe_declared_evidence(
                    source.bytes,
                    limits.content.max_file_bytes,
                ),
            })
            .collect(),
    })
}
