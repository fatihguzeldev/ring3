use super::pe_headers::admit_sources;
use super::{
    AsciiPeSource, AsciiPeSourceEvidenceError, AsciiPeSourceHeaderError, AsciiPeSourceHeaderLimits,
    AsciiSourcePathEntry, AsciiSourcePathLimits,
};
use crate::{
    PeFingerprintError, PeHeaderBatchLimits, PeModuleEvidence, PeModuleEvidenceLimits,
    PeModuleOutputLimits, inspect_pe_module_evidence,
};

/// path/content admission and the same per-family output caps for every occurrence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AsciiPeSourceModuleEvidenceLimits {
    pub paths: AsciiSourcePathLimits,
    pub content: PeHeaderBatchLimits,
    pub static_imports: PeModuleOutputLimits,
    pub delay_imports: PeModuleOutputLimits,
    pub exports: PeModuleOutputLimits,
}

/// owned observations for one caller-paired path and its content.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AsciiPeSourceModuleEvidence {
    pub path: AsciiSourcePathEntry,
    /// an algorithm-length refusal stays in this entry; later entries continue.
    pub module: Result<PeModuleEvidence, PeFingerprintError>,
}

/// owned results in input order; public fields are also caller-constructible.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AsciiPeSourceModuleEvidenceBatch {
    pub total_path_bytes: u64,
    /// logical bytes across all occurrences, counting aliases again and excluding paths.
    pub total_content_bytes: u64,
    pub entries: Vec<AsciiPeSourceModuleEvidence>,
}

/// admits every source, then collects owned same-input module evidence per entry.
///
/// shares path count, complete path admission and content count/size/checked-total
/// admission with the other named collectors. no hash or pe reader runs until
/// the whole list passes. zero limits are valid; outer refusals contain no batch.
///
/// the sole per-entry input cap comes from `content.max_file_bytes`. each source
/// starts fresh static/delay/export output accounting under the same family caps.
/// complete independent reader results and family refusals stay with that source;
/// neither a family refusal nor an entry fingerprint error stops later entries.
/// admitted empty or malformed bytes retain their digest and typed reader errors.
///
/// aliases count and are observed for every occurrence. paths do not participate
/// in digests, select formats, authenticate caller pairing or establish external
/// file/module identity. physical offsets refer to each paired input. path
/// allocation can precede content refusal. limits do not bound aggregate output,
/// process memory, time or acquisition, and do not provide cancellation.
///
/// # errors
/// returns the shared path or content admission error with its original operands.
///
/// # panics
/// only if admitted cardinality or an internal successful-reader invariant fails.
///
/// # example
/// ```
/// use ring3_core::{AsciiPeSource, AsciiPeSourceModuleEvidenceLimits,
///     AsciiSourcePathLimits, PeHeaderBatchLimits, PeModuleOutputLimits,
///     inspect_ascii_pe_source_module_evidence};
/// let output = PeModuleOutputLimits { max_rows: 0, max_text_bytes: 0 };
/// let batch = inspect_ascii_pe_source_module_evidence(
///     &[AsciiPeSource { path: "data", bytes: b"abc" }],
///     AsciiPeSourceModuleEvidenceLimits {
///         paths: AsciiSourcePathLimits {
///             max_paths: 1, max_path_bytes: 4, max_total_path_bytes: 4, max_depth: 1,
///         },
///         content: PeHeaderBatchLimits { max_files: 1, max_file_bytes: 3, max_total_bytes: 3 },
///         static_imports: output, delay_imports: output, exports: output,
///     },
/// )?;
/// assert_eq!(batch.entries[0].path.key, "data");
/// let module = batch.entries[0].module.as_ref().unwrap();
/// assert_eq!(module.fingerprinted.byte_length, 3);
/// assert!(module.fingerprinted.evidence.prefix.is_err());
/// assert!(module.static_imports.as_ref().unwrap().descriptors.is_err());
/// # Ok::<(), ring3_core::AsciiPeSourceEvidenceError>(())
/// ```
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn inspect_ascii_pe_source_module_evidence(
    sources: &[AsciiPeSource<'_>],
    limits: AsciiPeSourceModuleEvidenceLimits,
) -> Result<AsciiPeSourceModuleEvidenceBatch, AsciiPeSourceEvidenceError> {
    inspect_with(sources, limits, inspect_pe_module_evidence)
}

fn inspect_with(
    sources: &[AsciiPeSource<'_>],
    limits: AsciiPeSourceModuleEvidenceLimits,
    mut observe: impl FnMut(
        &[u8],
        PeModuleEvidenceLimits,
    ) -> Result<PeModuleEvidence, PeFingerprintError>,
) -> Result<AsciiPeSourceModuleEvidenceBatch, AsciiPeSourceEvidenceError> {
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
    let module_limits = PeModuleEvidenceLimits {
        max_input_bytes: limits.content.max_file_bytes,
        static_imports: limits.static_imports,
        delay_imports: limits.delay_imports,
        exports: limits.exports,
    };
    Ok(AsciiPeSourceModuleEvidenceBatch {
        total_path_bytes: admitted.total_path_bytes,
        total_content_bytes,
        entries: admitted
            .entries
            .into_iter()
            .zip(sources)
            .map(|(path, source)| AsciiPeSourceModuleEvidence {
                path,
                module: observe(source.bytes, module_limits),
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AsciiSourcePathError, PeHeaderBatchError};

    fn limits() -> AsciiPeSourceModuleEvidenceLimits {
        let output = PeModuleOutputLimits {
            max_rows: 3,
            max_text_bytes: 7,
        };
        AsciiPeSourceModuleEvidenceLimits {
            paths: AsciiSourcePathLimits {
                max_paths: 3,
                max_path_bytes: 10,
                max_total_path_bytes: 30,
                max_depth: 1,
            },
            content: PeHeaderBatchLimits {
                max_files: 3,
                max_file_bytes: 2,
                max_total_bytes: 6,
            },
            static_imports: output,
            delay_imports: output,
            exports: output,
        }
    }

    #[test]
    fn late_outer_refusals_never_observe_earlier_sources() {
        let first = AsciiPeSource {
            path: "first",
            bytes: b"a",
        };
        for (late, expected) in [
            (
                AsciiPeSource {
                    path: "",
                    bytes: b"abc",
                },
                AsciiPeSourceEvidenceError::Paths(AsciiSourcePathError::EmptyPath { index: 1 }),
            ),
            (
                AsciiPeSource {
                    path: "later",
                    bytes: b"abc",
                },
                AsciiPeSourceEvidenceError::Content(PeHeaderBatchError::FileSizeExceeded {
                    index: 1,
                    size: 3,
                    limit: 2,
                }),
            ),
        ] {
            let result = inspect_with(&[first, late], limits(), |_, _| {
                panic!("observer called before whole-list admission")
            });
            assert_eq!(result, Err(expected));
        }
    }

    #[test]
    fn observer_order_limits_and_entry_error_continuation_are_preserved() {
        let sources = [
            AsciiPeSource {
                path: "a",
                bytes: b"a",
            },
            AsciiPeSource {
                path: "b",
                bytes: b"bb",
            },
            AsciiPeSource {
                path: "alias",
                bytes: b"a",
            },
        ];
        let mut calls = Vec::new();
        let caps = limits();
        let module_limits = PeModuleEvidenceLimits {
            max_input_bytes: 2,
            static_imports: caps.static_imports,
            delay_imports: caps.delay_imports,
            exports: caps.exports,
        };
        // synthetic refusal tests only collector continuation, not an actual huge sha input.
        let injected = PeFingerprintError::Sha256LengthExceeded {
            length: 99,
            limit: 98,
        };
        let batch = inspect_with(&sources, caps, |bytes, limits| {
            let index = calls.len();
            calls.push(bytes.to_vec());
            assert_eq!(limits, module_limits);
            if index == 1 {
                Err(injected)
            } else {
                inspect_pe_module_evidence(bytes, limits)
            }
        })
        .unwrap();
        assert_eq!(calls, [b"a".to_vec(), b"bb".to_vec(), b"a".to_vec()]);
        assert_eq!(batch.total_content_bytes, 4);
        for (index, entry) in batch.entries.iter().enumerate() {
            assert_eq!(entry.path.index, index);
            assert_eq!(entry.path.normalized, sources[index].path);
            let expected = if index == 1 {
                Err(injected)
            } else {
                inspect_pe_module_evidence(sources[index].bytes, module_limits)
            };
            assert_eq!(entry.module, expected);
        }
    }
}
