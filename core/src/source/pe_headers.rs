use super::{
    AsciiSourcePathEntry, AsciiSourcePathError, AsciiSourcePathLimits, admit_ascii_source_paths,
};
use crate::{
    PeHeaderBatchError, PeHeaderBatchLimits, PeHeaderError, PeHeaderPrefix,
    parse_pe_header_prefix_batch,
};

/// a caller-supplied path/content pairing; no external provenance is verified.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AsciiPeSource<'a> {
    pub path: &'a str,
    pub bytes: &'a [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AsciiPeSourceHeaderLimits {
    pub paths: AsciiSourcePathLimits,
    pub headers: PeHeaderBatchLimits,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AsciiPeSourceHeaderError {
    Paths(AsciiSourcePathError),
    Headers(PeHeaderBatchError),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AsciiPeSourceHeader {
    pub path: AsciiSourcePathEntry,
    pub header: Result<PeHeaderPrefix, PeHeaderError>,
}

/// owned source results in input order, independent of both input lifetimes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AsciiPeSourceHeaders {
    pub total_path_bytes: u64,
    /// raw content bytes across all occurrences, excluding path text.
    pub total_content_bytes: u64,
    pub entries: Vec<AsciiPeSourceHeader>,
}

/// admits all paths before checking content budgets or reading any prefix.
///
/// the path count cap precedes reference projections; full path admission
/// precedes the independent header count/size/total preflight. admitted paths
/// and temporary reference vectors may be allocated before a content refusal.
/// logical byte limits do not bound allocation, acquisition or process memory.
///
/// every admitted source retains its supplied path/content pairing, regardless
/// of extension. malformed prefixes remain entry errors; later inputs continue.
/// repeated byte references count each time. path indices are input positions,
/// not content identity or source ids for a forwarder walk. this does not prove
/// filesystem containment, windows identity, provenance or image loadability.
///
/// # errors
/// returns the first path admission error, or the first header batch admission
/// error after every path is accepted. outer errors contain no partial result.
///
/// # example
/// ```
/// use ring3_core::{AsciiPeSource, AsciiPeSourceHeaderLimits, AsciiSourcePathLimits,
///     PeHeaderBatchLimits, parse_ascii_pe_source_headers};
/// let sources = [AsciiPeSource { path: "data", bytes: b"" }];
/// let result = parse_ascii_pe_source_headers(&sources, AsciiPeSourceHeaderLimits {
///     paths: AsciiSourcePathLimits {
///         max_paths: 1, max_path_bytes: 4, max_total_path_bytes: 4, max_depth: 1,
///     },
///     headers: PeHeaderBatchLimits { max_files: 1, max_file_bytes: 0, max_total_bytes: 0 },
/// })?;
/// assert_eq!(result.entries[0].path.key, "data");
/// assert!(result.entries[0].header.is_err());
/// assert_eq!(result.total_content_bytes, 0);
/// # Ok::<(), ring3_core::AsciiPeSourceHeaderError>(())
/// ```
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn parse_ascii_pe_source_headers(
    sources: &[AsciiPeSource<'_>],
    limits: AsciiPeSourceHeaderLimits,
) -> Result<AsciiPeSourceHeaders, AsciiPeSourceHeaderError> {
    let count = sources.len() as u64;
    if count > limits.paths.max_paths {
        return Err(AsciiPeSourceHeaderError::Paths(
            AsciiSourcePathError::PathCountExceeded {
                count,
                limit: limits.paths.max_paths,
            },
        ));
    }
    let paths: Vec<_> = sources.iter().map(|source| source.path).collect();
    let admitted =
        admit_ascii_source_paths(&paths, limits.paths).map_err(AsciiPeSourceHeaderError::Paths)?;
    let bytes: Vec<_> = sources.iter().map(|source| source.bytes).collect();
    let headers = parse_pe_header_prefix_batch(&bytes, limits.headers)
        .map_err(AsciiPeSourceHeaderError::Headers)?;
    Ok(AsciiPeSourceHeaders {
        total_path_bytes: admitted.total_path_bytes,
        total_content_bytes: headers.total_bytes,
        entries: admitted
            .entries
            .into_iter()
            .zip(headers.headers)
            .map(|(path, header)| AsciiPeSourceHeader { path, header })
            .collect(),
    })
}
