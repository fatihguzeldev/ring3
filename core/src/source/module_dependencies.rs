use std::collections::BTreeMap;

use super::application_candidates::admit_application_basename;
use crate::{
    AsciiApplicationSourceCandidateError, AsciiPeSourceModuleEvidenceBatch, AsciiSourcePathBatch,
    AsciiSourcePathError, AsciiSourcePathLimits, PeDelayImportEvidenceError,
    PeDelayImportNameError, PeFingerprintError, PeImportError, PeModuleEvidence,
    PeOwnedDelayImportName, PeOwnedImportDescriptor, PeStaticImportEvidenceError,
    admit_ascii_source_paths,
};

/// the retained view that supplied a request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeModuleDependencyKind {
    Static,
    Delay,
}

/// independent static family or descriptor-view refusal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeStaticDependencyError {
    Evidence(PeStaticImportEvidenceError),
    Descriptors(PeImportError),
}

/// independent delay family or name-view refusal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeDelayDependencyError {
    Evidence(PeDelayImportEvidenceError),
    Names(PeDelayImportNameError),
}

/// actual successful view lengths; delay absence differs from a present empty table.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeModuleDependencyViews {
    pub static_imports: Result<u64, PeStaticDependencyError>,
    pub delay_imports: Result<Option<u64>, PeDelayDependencyError>,
}

/// one exact request occurrence and its lexical candidate, not a resolved provider.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AsciiPeModuleDependencyRequest {
    pub source_index: usize,
    pub kind: PeModuleDependencyKind,
    pub descriptor_index: usize,
    pub dll_name: String,
    pub candidate: Result<Option<usize>, AsciiApplicationSourceCandidateError>,
}

/// owned paths, aligned source diagnostics and ordered request observations.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AsciiPeModuleDependencyEvidence {
    pub paths: AsciiSourcePathBatch,
    pub application_source_index: usize,
    pub total_requests: u64,
    pub total_request_text_bytes: u64,
    pub sources: Vec<Result<PeModuleDependencyViews, PeFingerprintError>>,
    pub requests: Vec<AsciiPeModuleDependencyRequest>,
}

/// path admission and logical request count/text caps, not memory or time limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AsciiPeModuleDependencyLimits {
    pub paths: AsciiSourcePathLimits,
    pub max_requests: u64,
    pub max_request_text_bytes: u64,
    pub max_basename_bytes: u64,
}

/// whole-report admission refusal; per-source and per-token errors remain nested.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AsciiPeModuleDependencyError {
    Paths(AsciiSourcePathError),
    ApplicationSourceIndexOutOfRange {
        index: usize,
        count: usize,
    },
    RequestCountOverflow {
        source_index: usize,
        kind: PeModuleDependencyKind,
        total: u64,
        count: u64,
    },
    RequestCountExceeded {
        count: u64,
        limit: u64,
    },
    RequestTextOverflow {
        source_index: usize,
        kind: PeModuleDependencyKind,
        descriptor_index: usize,
        total: u64,
        bytes: u64,
    },
    RequestTextExceeded {
        bytes: u64,
        limit: u64,
    },
}

enum Rows<'a> {
    Static(&'a [PeOwnedImportDescriptor]),
    Delay(&'a [PeOwnedDelayImportName]),
}

impl Rows<'_> {
    fn len(&self) -> usize {
        match self {
            Self::Static(rows) => rows.len(),
            Self::Delay(rows) => rows.len(),
        }
    }

    fn name(&self, index: usize) -> &str {
        match self {
            Self::Static(rows) => &rows[index].dll_name,
            Self::Delay(rows) => &rows[index].dll_name,
        }
    }
}

fn groups(module: &PeModuleEvidence) -> impl Iterator<Item = (PeModuleDependencyKind, Rows<'_>)> {
    let static_rows = module
        .static_imports
        .as_ref()
        .ok()
        .and_then(|e| e.descriptors.as_ref().ok())
        .map(|rows| Rows::Static(rows));
    let delay_rows = module
        .delay_imports
        .as_ref()
        .ok()
        .and_then(|e| e.names.as_ref().ok())
        .and_then(Option::as_ref)
        .map(|table| Rows::Delay(&table.imports));
    [
        (PeModuleDependencyKind::Static, static_rows),
        (PeModuleDependencyKind::Delay, delay_rows),
    ]
    .into_iter()
    .filter_map(|(kind, rows)| rows.map(|rows| (kind, rows)))
}

fn views(module: &PeModuleEvidence) -> PeModuleDependencyViews {
    PeModuleDependencyViews {
        static_imports: module
            .static_imports
            .as_ref()
            .map_err(|e| PeStaticDependencyError::Evidence(*e))
            .and_then(|e| {
                e.descriptors
                    .as_ref()
                    .map(|rows| rows.len() as u64)
                    .map_err(|e| PeStaticDependencyError::Descriptors(*e))
            }),
        delay_imports: module
            .delay_imports
            .as_ref()
            .map_err(|e| PeDelayDependencyError::Evidence(*e))
            .and_then(|e| {
                e.names
                    .as_ref()
                    .map(|table| table.as_ref().map(|t| t.imports.len() as u64))
                    .map_err(|e| PeDelayDependencyError::Names(*e))
            }),
    }
}

fn count_add(
    total: u64,
    count: u64,
    source_index: usize,
    kind: PeModuleDependencyKind,
) -> Result<u64, AsciiPeModuleDependencyError> {
    total
        .checked_add(count)
        .ok_or(AsciiPeModuleDependencyError::RequestCountOverflow {
            source_index,
            kind,
            total,
            count,
        })
}

fn text_add(
    total: u64,
    bytes: u64,
    source_index: usize,
    kind: PeModuleDependencyKind,
    descriptor_index: usize,
) -> Result<u64, AsciiPeModuleDependencyError> {
    total
        .checked_add(bytes)
        .ok_or(AsciiPeModuleDependencyError::RequestTextOverflow {
            source_index,
            kind,
            descriptor_index,
            total,
            bytes,
        })
}

// paths and the application index have already passed same-call admission.
fn application_candidate_index(
    paths: &AsciiSourcePathBatch,
    application_source_index: usize,
) -> BTreeMap<&str, usize> {
    let application = &paths.entries[application_source_index];
    let parent = application
        .key
        .rsplit_once('/')
        .map_or("", |(parent, _)| parent);
    paths
        .entries
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| {
            let (entry_parent, name) = entry.key.rsplit_once('/').unwrap_or(("", &entry.key));
            (entry_parent == parent).then_some((name, index))
        })
        .collect()
}

/// observes every retained static descriptor and delay name in one application context.
/// source and candidate indices are positions in the input entries vector; request
/// descriptor indices are occurrences within the successful static or delay view.
/// every actual path.normalized string is re-admitted. stored index/key/depth,
/// batch/family totals, fingerprints, lookups, exports and bound imports are not
/// authority. caller-constructible metadata does not authenticate path/content pairing.
///
/// admission order is source count, complete paths, application index, complete
/// checked request count, then complete checked raw utf-8 token byte count.
/// exact caps succeed; zero caps are valid. invalid or unmatched tokens still
/// count. token copies and candidate calls begin only after aggregate admission.
/// paths can allocate earlier; retained input allocations are outside these caps.
///
/// module/family/view errors remain independent. static empty, delay absent and
/// delay present-empty remain distinct. requests retain source, static-before-delay
/// and view order, including duplicates. requests share a same-call index of freshly
/// admitted application-parent basenames; zero requests do not build the index.
/// each basename is still admitted independently under the lexical finder's rules,
/// preserving its full error or optional candidate index. none does not mean a
/// missing windows dll. self/cyclic candidates
/// are observations; no recursive traversal, provider resolution or loading occurs.
///
/// output outlives input. no filesystem i/o, global memory/time bound, allocation
/// failure recovery, cancellation, format compatibility or loadability is provided.
///
/// ```
/// use ring3_core::{AsciiPeSourceModuleEvidenceBatch, AsciiPeSourceModuleEvidence,
///     AsciiSourcePathEntry, AsciiSourcePathLimits, AsciiPeModuleDependencyLimits,
///     PeFingerprintError, observe_ascii_pe_module_dependencies};
/// let failure = PeFingerprintError::InputTooLarge { length: 9, limit: 2 };
/// let output = {
///     let input = AsciiPeSourceModuleEvidenceBatch {
///         total_path_bytes: 999, total_content_bytes: 999,
///         entries: vec![AsciiPeSourceModuleEvidence {
///             path: AsciiSourcePathEntry {
///                 index: 99, normalized: "app.exe".into(), key: "ignored".into(), depth: 9,
///             }, module: Err(failure),
///         }],
///     };
///     observe_ascii_pe_module_dependencies(&input, 0, AsciiPeModuleDependencyLimits {
///         paths: AsciiSourcePathLimits {
///             max_paths: 1, max_path_bytes: 7, max_total_path_bytes: 7, max_depth: 1,
///         }, max_requests: 0, max_request_text_bytes: 0, max_basename_bytes: 0,
///     }).unwrap()
/// };
/// assert_eq!(output.paths.entries[0].index, 0);
/// assert_eq!(output.sources, vec![Err(failure)]);
/// assert!(output.requests.is_empty());
/// ```
///
/// # errors
/// returns the first path/index/count/text admission refusal without a partial report.
/// checked overflows preserve source/family/occurrence coordinates and operands.
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn observe_ascii_pe_module_dependencies(
    batch: &AsciiPeSourceModuleEvidenceBatch,
    application_source_index: usize,
    limits: AsciiPeModuleDependencyLimits,
) -> Result<AsciiPeModuleDependencyEvidence, AsciiPeModuleDependencyError> {
    use AsciiPeModuleDependencyError as Error;
    let count = batch.entries.len() as u64;
    if count > limits.paths.max_paths {
        return Err(Error::Paths(AsciiSourcePathError::PathCountExceeded {
            count,
            limit: limits.paths.max_paths,
        }));
    }
    let labels: Vec<&str> = batch
        .entries
        .iter()
        .map(|entry| entry.path.normalized.as_str())
        .collect();
    let paths = admit_ascii_source_paths(&labels, limits.paths).map_err(Error::Paths)?;
    if application_source_index >= paths.entries.len() {
        return Err(Error::ApplicationSourceIndexOutOfRange {
            index: application_source_index,
            count: paths.entries.len(),
        });
    }
    let mut total_requests = 0;
    for (source_index, entry) in batch.entries.iter().enumerate() {
        if let Ok(module) = &entry.module {
            for (kind, rows) in groups(module) {
                total_requests = count_add(total_requests, rows.len() as u64, source_index, kind)?;
            }
        }
    }
    if total_requests > limits.max_requests {
        return Err(Error::RequestCountExceeded {
            count: total_requests,
            limit: limits.max_requests,
        });
    }
    let mut total_request_text_bytes = 0;
    for (source_index, entry) in batch.entries.iter().enumerate() {
        if let Ok(module) = &entry.module {
            for (kind, rows) in groups(module) {
                for descriptor_index in 0..rows.len() {
                    total_request_text_bytes = text_add(
                        total_request_text_bytes,
                        rows.name(descriptor_index).len() as u64,
                        source_index,
                        kind,
                        descriptor_index,
                    )?;
                }
            }
        }
    }
    if total_request_text_bytes > limits.max_request_text_bytes {
        return Err(Error::RequestTextExceeded {
            bytes: total_request_text_bytes,
            limit: limits.max_request_text_bytes,
        });
    }
    let candidates: BTreeMap<&str, usize> = if total_requests == 0 {
        BTreeMap::new()
    } else {
        application_candidate_index(&paths, application_source_index)
    };
    let mut sources = Vec::new();
    let mut requests = Vec::new();
    for (source_index, entry) in batch.entries.iter().enumerate() {
        sources.push(entry.module.as_ref().map(views).map_err(|error| *error));
        if let Ok(module) = &entry.module {
            for (kind, rows) in groups(module) {
                for descriptor_index in 0..rows.len() {
                    let token = rows.name(descriptor_index);
                    let dll_name = token.to_owned();
                    let candidate = admit_application_basename(token, limits.max_basename_bytes)
                        .map(|token| candidates.get(token.key.as_str()).copied());
                    requests.push(AsciiPeModuleDependencyRequest {
                        source_index,
                        kind,
                        descriptor_index,
                        dll_name,
                        candidate,
                    });
                }
            }
        }
    }
    drop(candidates);
    Ok(AsciiPeModuleDependencyEvidence {
        paths,
        application_source_index,
        total_requests,
        total_request_text_bytes,
        sources,
        requests,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        AsciiPeModuleDependencyError as Error, PeModuleDependencyKind as Kind, count_add, text_add,
    };

    #[test]
    fn request_count_arithmetic_retains_exact_overflow_operands() {
        for kind in [Kind::Static, Kind::Delay] {
            for (total, count, expected) in [
                (0, 0, 0),
                (u64::MAX - 2, 2, u64::MAX),
                (u64::MAX, 0, u64::MAX),
            ] {
                assert_eq!(count_add(total, count, 3, kind), Ok(expected));
            }
            assert_eq!(
                count_add(u64::MAX, 1, 3, kind),
                Err(Error::RequestCountOverflow {
                    source_index: 3,
                    kind,
                    total: u64::MAX,
                    count: 1,
                })
            );
        }
    }

    #[test]
    fn request_text_arithmetic_retains_exact_overflow_coordinates() {
        for kind in [Kind::Static, Kind::Delay] {
            for (total, bytes, expected) in [
                (0, 0, 0),
                (u64::MAX - 2, 2, u64::MAX),
                (u64::MAX, 0, u64::MAX),
            ] {
                assert_eq!(text_add(total, bytes, 3, kind, 5), Ok(expected));
            }
            assert_eq!(
                text_add(u64::MAX, 1, 3, kind, 5),
                Err(Error::RequestTextOverflow {
                    source_index: 3,
                    kind,
                    descriptor_index: 5,
                    total: u64::MAX,
                    bytes: 1,
                })
            );
        }
    }
}
