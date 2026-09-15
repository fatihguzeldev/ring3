use super::pe_headers::admit_sources;
use super::{
    AsciiPeSource, AsciiPeSourceHeaderError, AsciiPeSourceHeaderLimits, AsciiSourcePathEntry,
    AsciiSourcePathError, AsciiSourcePathLimits,
};
use crate::{
    PeDeclaredEvidence, PeHeaderBatchError, PeHeaderBatchLimits, inspect_pe_declared_evidence,
};

/// independent logical path and content budgets for already materialized inputs.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AsciiPeSourceEvidenceLimits {
    pub paths: AsciiSourcePathLimits,
    pub content: PeHeaderBatchLimits,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AsciiPeSourceEvidenceError {
    Paths(AsciiSourcePathError),
    Content(PeHeaderBatchError),
}

/// owned evidence for the content paired with this caller-supplied path.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AsciiPeSourceEvidence {
    pub path: AsciiSourcePathEntry,
    /// physical field offsets are relative to this source's input bytes.
    pub evidence: PeDeclaredEvidence,
}

/// owned results in input order; indices are positions, not stable source ids.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AsciiPeSourceEvidenceBatch {
    pub total_path_bytes: u64,
    /// content bytes across all occurrences, counting aliases again and excluding paths.
    pub total_content_bytes: u64,
    pub entries: Vec<AsciiPeSourceEvidence>,
}

/// admits paired sources and retains all three reader outcomes for every entry.
///
/// shares path and content admission with the header collector:
/// path count precedes projections, complete path admission precedes content
/// count/size/total checks, and no pe bytes are read before admission completes.
/// zero limits are valid. outer refusals contain no partial result.
///
/// each admitted source uses `inspect_pe_declared_evidence` directly, with its
/// independent reader work. paths and temporary vectors may be allocated before
/// a content refusal. logical budgets do not cap acquisition or process memory.
///
/// malformed sections remain entry-local outcomes and do not stop later sources.
/// filenames do not select formats. caller pairing, lexical keys and input indices
/// do not establish external provenance, file identity or windows resolution.
/// declarations do not establish loadability, runtime requirements or support.
///
/// # errors
/// preserves the first path admission error, or maps the first header batch
/// admission error to `Content` with unchanged operands after all paths pass.
///
/// # panics
/// only if delegated result cardinality or an internal successful-reader invariant
/// is violated. malformed input is represented by admission or per-entry errors.
///
/// # example
/// ```
/// use ring3_core::{AsciiPeSource, AsciiPeSourceEvidenceLimits, AsciiSourcePathLimits,
///     PeHeaderBatchLimits, inspect_ascii_pe_source_evidence};
/// let batch = inspect_ascii_pe_source_evidence(
///     &[AsciiPeSource { path: "data", bytes: b"" }],
///     AsciiPeSourceEvidenceLimits {
///         paths: AsciiSourcePathLimits {
///             max_paths: 1, max_path_bytes: 4, max_total_path_bytes: 4, max_depth: 1,
///         },
///         content: PeHeaderBatchLimits { max_files: 1, max_file_bytes: 0, max_total_bytes: 0 },
///     },
/// )?;
/// assert_eq!(batch.entries[0].path.key, "data");
/// assert!(batch.entries[0].evidence.prefix.is_err());
/// assert!(batch.entries[0].evidence.optional.is_err());
/// assert!(batch.entries[0].evidence.clr.is_err());
/// # Ok::<(), ring3_core::AsciiPeSourceEvidenceError>(())
/// ```
#[expect(
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    reason = "project documentation headings are lower case"
)]
pub fn inspect_ascii_pe_source_evidence(
    sources: &[AsciiPeSource<'_>],
    limits: AsciiPeSourceEvidenceLimits,
) -> Result<AsciiPeSourceEvidenceBatch, AsciiPeSourceEvidenceError> {
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
    Ok(AsciiPeSourceEvidenceBatch {
        total_path_bytes: admitted.total_path_bytes,
        total_content_bytes,
        entries: admitted
            .entries
            .into_iter()
            .zip(sources)
            .map(|(path, source)| AsciiPeSourceEvidence {
                path,
                evidence: inspect_pe_declared_evidence(source.bytes),
            })
            .collect(),
    })
}
