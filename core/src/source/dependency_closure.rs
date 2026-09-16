use crate::{
    AsciiPeModuleDependencyError, AsciiPeModuleDependencyEvidence, AsciiPeModuleDependencyLimits,
    AsciiPeSourceModuleEvidenceBatch, PeModuleDependencyKind, observe_ascii_pe_module_dependencies,
};

/// which observed edges contribute to the potential reachable component.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeDependencyClosureMode {
    /// follow static requests and retain excluded delay occurrences.
    StaticOnly,
    /// follow static and delay requests; neither implies launch-time necessity.
    StaticAndDelay,
}

/// a source vector position in first-discovery order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeDependencyVisit {
    pub source_index: usize,
    /// original observation request index; absent only for the root.
    pub via_request_index: Option<usize>,
}

/// classification of one examined occurrence; a revisit alone is not a cycle.
/// each `visit_index` addresses the returned closure's `visits` vector.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeDependencyRequestStep {
    ExcludedDelay,
    NoCandidate,
    CandidateError,
    AlreadyReached { visit_index: usize },
    Discovered { visit_index: usize },
}

/// an original observation request index and its traversal result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeDependencyRequestVisit {
    pub request_index: usize,
    pub step: PeDependencyRequestStep,
}

/// owned whole-inventory observations and the selected root's component.
/// vector lengths are the authoritative traversal counts.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AsciiPeDependencyClosure {
    pub observations: AsciiPeModuleDependencyEvidence,
    pub mode: PeDependencyClosureMode,
    pub visits: Vec<PeDependencyVisit>,
    pub examined_requests: Vec<PeDependencyRequestVisit>,
}

/// whole-inventory observation caps followed by logical traversal caps.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AsciiPeDependencyClosureLimits {
    pub observation: AsciiPeModuleDependencyLimits,
    /// includes the root and every newly reached source vector position.
    pub max_reached_sources: u64,
    /// includes excluded delay, missing/error candidates and revisits.
    pub max_examined_requests: u64,
}

/// whole-observation or traversal refusal; no partial closure is returned.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AsciiPeDependencyClosureError {
    Observation(AsciiPeModuleDependencyError),
    ReachedSourcesExceeded {
        source_index: usize,
        via_request_index: Option<usize>,
        count: u64,
        limit: u64,
    },
    ExaminedRequestsExceeded {
        source_index: usize,
        request_index: usize,
        count: u64,
        limit: u64,
    },
}

fn next_count(used: usize, total: usize) -> u64 {
    // each next item is unique within an actual source or request vector.
    assert!(used < total, "next item belongs to the actual vector");
    u64::try_from(used)
        .expect("supported source length fits u64")
        .checked_add(1)
        .expect("next item is bounded by a u64 vector length")
}

/// visits the selected application's observed lexical component in breadth-first order.
///
/// the existing observer admits the whole inventory once before traversal, including
/// unrelated sources and requests. the application context is also the root. each
/// source vector position is visited once; equal fingerprints or bytes do not merge
/// occurrences. outgoing requests retain their original report order and flat indices.
/// first discovery supplies each non-root visit's predecessor request.
///
/// root admission precedes request admission. every examined request counts before
/// classification; its cap precedes any new-target reached cap. static-only mode
/// excludes delay edges before inspecting their candidates. complete owned observations
/// retain all diagnostics, including errors on unreachable sources.
///
/// traversal limits do not undo observer work. range/visit indexes cover the admitted
/// inventory. these are logical count limits, not global memory/time, cancellation or
/// allocation-failure guarantees. neither mode proves launch requirements, physical
/// module identity, windows resolution or loadability. no guest code is executed.
///
/// # errors
///
/// returns the unchanged observer error first, then a reached-source or examined-request
/// cap refusal with the attempted count. outer refusal returns no partial result.
///
/// # panics
///
/// panics if supported-target length conversion or internally produced observation
/// index/count invariants are violated. admitted caller metadata cannot violate these
/// invariants on the supported native64 and wasm32 targets.
///
/// ```
/// use ring3_core::{
///     AsciiPeDependencyClosureError, AsciiPeDependencyClosureLimits,
///     AsciiPeModuleDependencyError, AsciiPeModuleDependencyLimits,
///     AsciiPeSourceModuleEvidenceBatch, AsciiSourcePathLimits, PeDependencyClosureMode,
///     walk_ascii_pe_dependency_closure,
/// };
/// let batch = AsciiPeSourceModuleEvidenceBatch {
///     total_path_bytes: 0, total_content_bytes: 0, entries: vec![],
/// };
/// let limits = AsciiPeDependencyClosureLimits {
///     observation: AsciiPeModuleDependencyLimits {
///         paths: AsciiSourcePathLimits {
///             max_paths: 0, max_path_bytes: 0, max_total_path_bytes: 0, max_depth: 0,
///         },
///         max_requests: 0, max_request_text_bytes: 0, max_basename_bytes: 0,
///     },
///     max_reached_sources: 0, max_examined_requests: 0,
/// };
/// assert_eq!(
///     walk_ascii_pe_dependency_closure(&batch, 0, PeDependencyClosureMode::StaticOnly, limits),
///     Err(AsciiPeDependencyClosureError::Observation(
///         AsciiPeModuleDependencyError::ApplicationSourceIndexOutOfRange { index: 0, count: 0 },
///     )),
/// );
/// ```
#[expect(
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    reason = "project documentation headings are lower case"
)]
pub fn walk_ascii_pe_dependency_closure(
    batch: &AsciiPeSourceModuleEvidenceBatch,
    application_source_index: usize,
    mode: PeDependencyClosureMode,
    limits: AsciiPeDependencyClosureLimits,
) -> Result<AsciiPeDependencyClosure, AsciiPeDependencyClosureError> {
    use AsciiPeDependencyClosureError::{
        ExaminedRequestsExceeded, Observation, ReachedSourcesExceeded,
    };
    let observations =
        observe_ascii_pe_module_dependencies(batch, application_source_index, limits.observation)
            .map_err(Observation)?;
    let source_count = observations.sources.len();
    let request_count = observations.requests.len();
    u64::try_from(source_count).expect("supported source length fits u64");
    u64::try_from(request_count).expect("supported request length fits u64");
    if limits.max_reached_sources == 0 {
        return Err(ReachedSourcesExceeded {
            source_index: application_source_index,
            via_request_index: None,
            count: 1,
            limit: 0,
        });
    }
    let mut ranges = vec![0..0; source_count];
    for (request_index, request) in observations.requests.iter().enumerate() {
        let range = &mut ranges[request.source_index];
        if range.start == range.end {
            range.start = request_index;
        }
        range.end = request_index + 1;
    }
    let mut reached = vec![None; source_count];
    reached[application_source_index] = Some(0);
    let mut visits = vec![PeDependencyVisit {
        source_index: application_source_index,
        via_request_index: None,
    }];
    let mut examined_requests = Vec::new();
    let mut cursor = 0;
    while cursor < visits.len() {
        let source_index = visits[cursor].source_index;
        for request_index in ranges[source_index].clone() {
            let count = next_count(examined_requests.len(), request_count);
            if count > limits.max_examined_requests {
                return Err(ExaminedRequestsExceeded {
                    source_index,
                    request_index,
                    count,
                    limit: limits.max_examined_requests,
                });
            }
            let request = &observations.requests[request_index];
            let step = if mode == PeDependencyClosureMode::StaticOnly
                && request.kind == PeModuleDependencyKind::Delay
            {
                PeDependencyRequestStep::ExcludedDelay
            } else {
                match request.candidate {
                    Err(_) => PeDependencyRequestStep::CandidateError,
                    Ok(None) => PeDependencyRequestStep::NoCandidate,
                    Ok(Some(target)) => {
                        if let Some(visit_index) = reached[target] {
                            PeDependencyRequestStep::AlreadyReached { visit_index }
                        } else {
                            let count = next_count(visits.len(), source_count);
                            if count > limits.max_reached_sources {
                                return Err(ReachedSourcesExceeded {
                                    source_index: target,
                                    via_request_index: Some(request_index),
                                    count,
                                    limit: limits.max_reached_sources,
                                });
                            }
                            let visit_index = visits.len();
                            reached[target] = Some(visit_index);
                            visits.push(PeDependencyVisit {
                                source_index: target,
                                via_request_index: Some(request_index),
                            });
                            PeDependencyRequestStep::Discovered { visit_index }
                        }
                    }
                }
            };
            examined_requests.push(PeDependencyRequestVisit {
                request_index,
                step,
            });
        }
        cursor += 1;
    }
    Ok(AsciiPeDependencyClosure {
        observations,
        mode,
        visits,
        examined_requests,
    })
}
