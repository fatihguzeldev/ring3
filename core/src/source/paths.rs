use std::collections::BTreeMap;

/// explicit limits on already materialized raw path text; no default namespace policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AsciiSourcePathLimits {
    pub max_paths: u64,
    pub max_path_bytes: u64,
    pub max_total_path_bytes: u64,
    pub max_depth: u64,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AsciiSourcePathSegmentError {
    Empty,
    Dot,
    Parent,
    TrailingDotOrSpace,
    ReservedDeviceStem,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AsciiSourcePathCollision {
    Duplicate,
    AncestorFile,
    DescendantFile,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AsciiSourcePathError {
    PathCountExceeded {
        count: u64,
        limit: u64,
    },
    PathBytesExceeded {
        index: usize,
        bytes: u64,
        limit: u64,
    },
    TotalPathBytesOverflow {
        index: usize,
        total: u64,
        bytes: u64,
    },
    TotalPathBytesExceeded {
        index: usize,
        total: u64,
        limit: u64,
    },
    EmptyPath {
        index: usize,
    },
    AbsolutePath {
        index: usize,
    },
    NonAsciiByte {
        index: usize,
        offset: usize,
    },
    ForbiddenByte {
        index: usize,
        offset: usize,
        byte: u8,
    },
    DepthExceeded {
        index: usize,
        depth: u64,
        limit: u64,
    },
    Segment {
        index: usize,
        segment: usize,
        reason: AsciiSourcePathSegmentError,
    },
    Collision {
        index: usize,
        prior: usize,
        kind: AsciiSourcePathCollision,
    },
}
/// owned lexical spelling/key and the original input-list position, not file identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AsciiSourcePathEntry {
    pub index: usize,
    pub normalized: String,
    pub key: String,
    pub depth: u64,
}
/// complete ordered results; output strings may outlive every input string.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AsciiSourcePathBatch {
    pub total_path_bytes: u64,
    pub entries: Vec<AsciiSourcePathEntry>,
}

fn charge_path_bytes(
    index: usize,
    total: u64,
    bytes: u64,
    limit: u64,
) -> Result<u64, AsciiSourcePathError> {
    let total = total
        .checked_add(bytes)
        .ok_or(AsciiSourcePathError::TotalPathBytesOverflow {
            index,
            total,
            bytes,
        })?;
    if total > limit {
        return Err(AsciiSourcePathError::TotalPathBytesExceeded {
            index,
            total,
            limit,
        });
    }
    Ok(total)
}
fn preflight(paths: &[&str], limits: AsciiSourcePathLimits) -> Result<u64, AsciiSourcePathError> {
    let count = paths.len() as u64;
    if count > limits.max_paths {
        return Err(AsciiSourcePathError::PathCountExceeded {
            count,
            limit: limits.max_paths,
        });
    }
    let mut total = 0;
    for (index, path) in paths.iter().enumerate() {
        let bytes = path.len() as u64;
        if bytes > limits.max_path_bytes {
            return Err(AsciiSourcePathError::PathBytesExceeded {
                index,
                bytes,
                limit: limits.max_path_bytes,
            });
        }
        total = charge_path_bytes(index, total, bytes, limits.max_total_path_bytes)?;
    }
    Ok(total)
}
fn reserved(part: &str) -> bool {
    let stem = part.split('.').next().unwrap().trim_end_matches(' ');
    ["CON", "PRN", "AUX", "NUL"]
        .iter()
        .any(|name| stem.eq_ignore_ascii_case(name))
        || (stem.len() == 4
            && (stem[..3].eq_ignore_ascii_case("COM") || stem[..3].eq_ignore_ascii_case("LPT"))
            && (b'1'..=b'9').contains(&stem.as_bytes()[3]))
}
fn normalize(
    index: usize,
    path: &str,
    max_depth: u64,
) -> Result<AsciiSourcePathEntry, AsciiSourcePathError> {
    if path.is_empty() {
        return Err(AsciiSourcePathError::EmptyPath { index });
    }
    if matches!(path.as_bytes()[0], b'/' | b'\\') {
        return Err(AsciiSourcePathError::AbsolutePath { index });
    }
    for (offset, byte) in path.bytes().enumerate() {
        if !byte.is_ascii() {
            return Err(AsciiSourcePathError::NonAsciiByte { index, offset });
        }
        if byte < 32 || byte == 127 || b"<>:\"|?*".contains(&byte) {
            return Err(AsciiSourcePathError::ForbiddenByte {
                index,
                offset,
                byte,
            });
        }
    }
    let depth = path.bytes().filter(|b| matches!(b, b'/' | b'\\')).count() as u64 + 1;
    if depth > max_depth {
        return Err(AsciiSourcePathError::DepthExceeded {
            index,
            depth,
            limit: max_depth,
        });
    }
    let normalized = path.replace('\\', "/");
    for (segment, part) in normalized.split('/').enumerate() {
        let reason = if part.is_empty() {
            Some(AsciiSourcePathSegmentError::Empty)
        } else if part == "." {
            Some(AsciiSourcePathSegmentError::Dot)
        } else if part == ".." {
            Some(AsciiSourcePathSegmentError::Parent)
        } else if part.ends_with(['.', ' ']) {
            Some(AsciiSourcePathSegmentError::TrailingDotOrSpace)
        } else if reserved(part) {
            Some(AsciiSourcePathSegmentError::ReservedDeviceStem)
        } else {
            None
        };
        if let Some(reason) = reason {
            return Err(AsciiSourcePathError::Segment {
                index,
                segment,
                reason,
            });
        }
    }
    let key = normalized.to_ascii_lowercase();
    Ok(AsciiSourcePathEntry {
        index,
        normalized,
        key,
        depth,
    })
}
pub(super) fn admit_ascii_source_basename(
    basename: &str,
    max_basename_bytes: u64,
) -> Result<AsciiSourcePathEntry, AsciiSourcePathError> {
    preflight(
        &[basename],
        AsciiSourcePathLimits {
            max_paths: 1,
            max_path_bytes: max_basename_bytes,
            max_total_path_bytes: max_basename_bytes,
            max_depth: 1,
        },
    )?;
    normalize(0, basename, 1)
}

/// admits a conservative ascii-only list of relative file paths with implicit directories.
/// changes only backslash separators to slash; comparison keys fold ascii case.
/// percent text stays literal. keys and indices are local lexical results, not
/// filesystem containment, content identity, windows names or pe source bindings.
/// non-ascii refusal does not imply that windows rejects the original name.
///
/// all raw path bytes are admitted before lexical inspection or result allocation.
/// result/index keys may be copied; limits do not bound acquisition, allocator
/// capacity, process memory, filesystem access or cancellation. zero limits and
/// an empty list are valid. this operation performs no file or directory access.
///
/// # errors
/// checks count, then every path's byte cap/total overflow/total cap in input order.
/// each path then checks empty/absolute, all raw bytes, depth and segment rules.
/// segments reject empty/dot/parent, trailing dot or space, then reserved device
/// stems: con/prn/aux/nul and com1..9/lpt1..9 before the first dot, ignoring ascii
/// case and trailing spaces in that stem. no dot collapsing or percent decoding.
/// collisions check duplicate, shallowest prior-file ancestor, then lexical first
/// prior descendant. shared implicit directories are valid. errors return no partial
/// entries; offsets count raw input bytes and segment indices are zero-based.
///
/// ```
/// use ring3_core::{AsciiSourcePathLimits, admit_ascii_source_paths};
/// let result = {
///     let input = String::from(r"Data\LEVEL.bin");
///     admit_ascii_source_paths(&[&input], AsciiSourcePathLimits {
///         max_paths: 1, max_path_bytes: 14, max_total_path_bytes: 14, max_depth: 2,
///     })?
/// };
/// assert_eq!(result.entries[0].normalized, "Data/LEVEL.bin");
/// assert_eq!(result.entries[0].key, "data/level.bin");
/// # Ok::<(), ring3_core::AsciiSourcePathError>(())
/// ```
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn admit_ascii_source_paths(
    paths: &[&str],
    limits: AsciiSourcePathLimits,
) -> Result<AsciiSourcePathBatch, AsciiSourcePathError> {
    let total_path_bytes = preflight(paths, limits)?;
    let mut seen = BTreeMap::<String, usize>::new();
    let mut entries = Vec::new();
    for (index, path) in paths.iter().enumerate() {
        let entry = normalize(index, path, limits.max_depth)?;
        let key = &entry.key;
        if let Some(&prior) = seen.get(key) {
            return Err(AsciiSourcePathError::Collision {
                index,
                prior,
                kind: AsciiSourcePathCollision::Duplicate,
            });
        }
        for (slash, _) in key.match_indices('/') {
            if let Some(&prior) = seen.get(&key[..slash]) {
                return Err(AsciiSourcePathError::Collision {
                    index,
                    prior,
                    kind: AsciiSourcePathCollision::AncestorFile,
                });
            }
        }
        let prefix = format!("{key}/");
        if let Some((previous, &prior)) = seen.range(prefix.clone()..).next()
            && previous.starts_with(&prefix)
        {
            return Err(AsciiSourcePathError::Collision {
                index,
                prior,
                kind: AsciiSourcePathCollision::DescendantFile,
            });
        }
        seen.insert(key.clone(), index);
        entries.push(entry);
    }
    Ok(AsciiSourcePathBatch {
        total_path_bytes,
        entries,
    })
}

#[cfg(test)]
mod tests {
    use super::{AsciiSourcePathError, charge_path_bytes};

    #[test]
    fn synthetic_total_byte_arithmetic_preserves_exact_limits_and_overflow_priority() {
        for (total, bytes, limit, expected) in [
            (0, 0, 0, Ok(0)),
            (0, 1, 0, Err((false, 1, 0))),
            (u64::MAX, 0, u64::MAX, Ok(u64::MAX)),
            (u64::MAX - 1, 1, u64::MAX, Ok(u64::MAX)),
            (u64::MAX, 1, u64::MAX, Err((true, u64::MAX, 1))),
            (u64::MAX - 1, 2, u64::MAX, Err((true, u64::MAX - 1, 2))),
            (0, u64::MAX, u64::MAX, Ok(u64::MAX)),
            (
                0,
                u64::MAX,
                u64::MAX - 1,
                Err((false, u64::MAX, u64::MAX - 1)),
            ),
            (1, u64::MAX, u64::MAX, Err((true, 1, u64::MAX))),
            (0, 0, u64::MAX, Ok(0)),
        ] {
            let expected = expected.map_err(|(overflow, total, bound)| {
                if overflow {
                    AsciiSourcePathError::TotalPathBytesOverflow {
                        index: 7,
                        total,
                        bytes: bound,
                    }
                } else {
                    AsciiSourcePathError::TotalPathBytesExceeded {
                        index: 7,
                        total,
                        limit: bound,
                    }
                }
            });
            assert_eq!(charge_path_bytes(7, total, bytes, limit), expected);
        }
    }
}
