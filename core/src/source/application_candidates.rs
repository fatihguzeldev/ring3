use super::{
    AsciiSourcePathBatch, AsciiSourcePathEntry, AsciiSourcePathError, AsciiSourcePathLimits,
    admit_ascii_source_paths,
};

/// explicit limits on already materialized paths and one literal basename.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AsciiApplicationSourceCandidateLimits {
    pub paths: AsciiSourcePathLimits,
    pub max_basename_bytes: u64,
}

/// failures preserve path admission, application-index, then basename precedence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AsciiApplicationSourceCandidateError {
    Paths(AsciiSourcePathError),
    ApplicationSourceIndexOutOfRange { index: usize, count: usize },
    Basename(AsciiSourcePathError),
}

/// finds one lexical sibling of the caller-designated application source.
///
/// admits the complete raw path list before checking the application index, then
/// admits the basename with depth one and both byte caps set to `max_basename_bytes`.
/// matches complete parent and basename keys; no recursion or extension synthesis.
/// full-key collision rejection guarantees at most one result. the selected entry
/// retains its original index, normalized spelling, key and depth and owns its text.
///
/// the context file need not be executable and may match itself. this does not
/// inspect content, select a loadable provider, authenticate filesystem identity or
/// implement windows search order. path and token admission may allocate metadata
/// before a later refusal; these limits are not a global memory or time bound.
/// performs no filesystem access and provides no allocation-failure recovery or
/// cancellation.
///
/// # errors
/// returns complete path-admission errors first, then an out-of-range application
/// index, then basename-admission errors with diagnostic index zero. an empty path
/// list has no valid application index. every refusal returns no partial candidate.
///
/// # panics
/// only if the existing admission owner violates its successful cardinality invariant.
///
/// ```
/// use ring3_core::{
///     AsciiApplicationSourceCandidateLimits, AsciiSourcePathLimits,
///     find_ascii_application_source_candidate,
/// };
/// let candidate = {
///     let paths = [String::from("bin/app.exe"), String::from("bin/A.DLL")];
///     let basename = String::from("a.dll");
///     find_ascii_application_source_candidate(
///         &paths.each_ref().map(String::as_str), 0, &basename,
///         AsciiApplicationSourceCandidateLimits {
///             paths: AsciiSourcePathLimits {
///                 max_paths: 2, max_path_bytes: 32,
///                 max_total_path_bytes: 64, max_depth: 2,
///             },
///             max_basename_bytes: 16,
///         },
///     ).unwrap().unwrap()
/// };
/// assert_eq!(candidate.index, 1);
/// assert_eq!(candidate.normalized, "bin/A.DLL");
/// assert_eq!(candidate.key, "bin/a.dll");
/// ```
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn find_ascii_application_source_candidate(
    paths: &[&str],
    application_source_index: usize,
    basename: &str,
    limits: AsciiApplicationSourceCandidateLimits,
) -> Result<Option<AsciiSourcePathEntry>, AsciiApplicationSourceCandidateError> {
    let mut admitted = admit_ascii_source_paths(paths, limits.paths)
        .map_err(AsciiApplicationSourceCandidateError::Paths)?;
    let position = find_admitted_application_source_position(
        &admitted,
        application_source_index,
        basename,
        limits.max_basename_bytes,
    )?;
    Ok(position.map(|index| admitted.entries.swap_remove(index)))
}

// paths must be fresh admission output, not caller-constructed metadata.
pub(super) fn find_admitted_application_source_position(
    admitted: &AsciiSourcePathBatch,
    application_source_index: usize,
    basename: &str,
    max_basename_bytes: u64,
) -> Result<Option<usize>, AsciiApplicationSourceCandidateError> {
    let application = admitted.entries.get(application_source_index).ok_or(
        AsciiApplicationSourceCandidateError::ApplicationSourceIndexOutOfRange {
            index: application_source_index,
            count: admitted.entries.len(),
        },
    )?;
    let token = admit_ascii_source_paths(
        &[basename],
        AsciiSourcePathLimits {
            max_paths: 1,
            max_path_bytes: max_basename_bytes,
            max_total_path_bytes: max_basename_bytes,
            max_depth: 1,
        },
    )
    .map_err(AsciiApplicationSourceCandidateError::Basename)?;
    let parent = application
        .key
        .rsplit_once('/')
        .map_or("", |(parent, _)| parent);
    Ok(admitted.entries.iter().position(|entry| {
        let (entry_parent, entry_name) = entry.key.rsplit_once('/').unwrap_or(("", &entry.key));
        entry_parent == parent && entry_name == token.entries[0].key
    }))
}
