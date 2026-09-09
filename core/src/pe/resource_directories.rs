use super::optional::{read_u16, read_u32};
use super::resources::parse_prepared_resource_root;
use super::rva::PreparedPe;
use super::{PeKind, PeResourceRoot, PeResourceRootError, PeRvaError};
use crate::{FileOffset, RelativeVirtualAddress};

const DIRECTORY_LIMIT: u16 = 256;
const ENTRY_LIMIT: u16 = 256;
const TOTAL_ENTRY_LIMIT: u32 = 4096;
const DEPTH_LIMIT: u16 = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeResourceDirectoryEntry {
    pub entry_rva: RelativeVirtualAddress,
    pub entry_file_offset: FileOffset,
    pub raw_name_or_id: u32,
    pub raw_data_or_subdirectory: u32,
    pub child_directory_index: Option<u16>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeResourceDirectory {
    pub directory_offset: u32,
    pub directory_rva: RelativeVirtualAddress,
    pub directory_file_offset: FileOffset,
    pub characteristics: u32,
    pub time_date_stamp: u32,
    pub major_version: u16,
    pub minor_version: u16,
    pub number_of_named_entries: u16,
    pub number_of_id_entries: u16,
    pub entries: Vec<PeResourceDirectoryEntry>,
    pub longest_root_path: u16,
}

/// owned raw directory graph; root is index zero and shared offsets use one index.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeResourceDirectoryGraph {
    pub kind: PeKind,
    pub directory_rva: RelativeVirtualAddress,
    pub directory_file_offset: FileOffset,
    pub directory_size: u32,
    pub directories: Vec<PeResourceDirectory>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeResourceDirectoryError {
    Root(PeResourceRootError),
    DirectoryLimitExceeded {
        offset: u32,
        count: u32,
        limit: u16,
    },
    DirectoryOutsideResource {
        offset: u32,
        length: u32,
        directory_size: u32,
    },
    DirectoryRange {
        offset: u32,
        start: RelativeVirtualAddress,
        length: u32,
        cause: PeRvaError,
    },
    EntryLimitExceeded {
        directory_offset: u32,
        count: u32,
        limit: u16,
    },
    TotalEntryLimitExceeded {
        directory_offset: u32,
        count: u32,
        limit: u32,
    },
    /// includes descendants blocked by cycles; not the length of a single cycle.
    Cycle {
        remaining_directories: u16,
    },
    DepthLimitExceeded {
        directory_offset: u32,
        depth: u16,
        limit: u16,
    },
}

/// reads an acyclic graph of raw resource directories through one prepared input.
/// supports at most 256 unique directories, 256 entries each, 4096 total entries
/// and a longest root path of 16 edges. these are reader limits, not format rules.
/// names, leaf data records and payloads remain unread; this is not a full resource
/// validation. directory identities follow first discovery in raw entry order.
///
/// # errors
/// validates the base and raw root first. new child identities are bounded before
/// reading. each child checks its declared header extent, header backing, widened
/// count and total budget, declared entry extent, then complete prefix backing.
/// after discovery, rejects cycles before checking the longest root path.
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn parse_pe_resource_directories(
    bytes: &[u8],
) -> Result<Option<PeResourceDirectoryGraph>, PeResourceDirectoryError> {
    let prepared = PreparedPe::new(bytes)
        .map_err(|cause| PeResourceDirectoryError::Root(PeResourceRootError::Base(cause)))?;
    parse_prepared_resource_directories(&prepared)
}

pub(super) fn parse_prepared_resource_directories(
    prepared: &PreparedPe<'_>,
) -> Result<Option<PeResourceDirectoryGraph>, PeResourceDirectoryError> {
    let Some(root) =
        parse_prepared_resource_root(prepared).map_err(PeResourceDirectoryError::Root)?
    else {
        return Ok(None);
    };
    let mut total = u32::from(root.number_of_named_entries) + u32::from(root.number_of_id_entries);
    let mut graph = PeResourceDirectoryGraph {
        kind: root.kind,
        directory_rva: root.directory_rva,
        directory_file_offset: root.directory_file_offset,
        directory_size: root.directory_size,
        directories: vec![root_directory(root)],
    };
    let mut offsets = vec![0];
    let mut count = 1_u16;
    let mut index = 0;
    while index < offsets.len() {
        if index != 0 {
            let directory = read_child(prepared, &graph, offsets[index], total)?;
            total += u32::from(directory.number_of_named_entries)
                + u32::from(directory.number_of_id_entries);
            graph.directories.push(directory);
        }
        for entry in &mut graph.directories[index].entries {
            if entry.raw_data_or_subdirectory & 0x8000_0000 == 0 {
                continue;
            }
            let offset = entry.raw_data_or_subdirectory & 0x7fff_ffff;
            let known = (0_u16..)
                .zip(&offsets)
                .find_map(|(i, &known)| (known == offset).then_some(i));
            let child = if let Some(known) = known {
                known
            } else {
                if count == DIRECTORY_LIMIT {
                    return Err(PeResourceDirectoryError::DirectoryLimitExceeded {
                        offset,
                        count: u32::from(count) + 1,
                        limit: DIRECTORY_LIMIT,
                    });
                }
                offsets.push(offset);
                let child = count;
                count += 1;
                child
            };
            entry.child_directory_index = Some(child);
        }
        index += 1;
    }
    validate_paths(&mut graph.directories, count)?;
    Ok(Some(graph))
}

fn root_directory(root: PeResourceRoot) -> PeResourceDirectory {
    PeResourceDirectory {
        directory_offset: 0,
        directory_rva: root.directory_rva,
        directory_file_offset: root.directory_file_offset,
        characteristics: root.characteristics,
        time_date_stamp: root.time_date_stamp,
        major_version: root.major_version,
        minor_version: root.minor_version,
        number_of_named_entries: root.number_of_named_entries,
        number_of_id_entries: root.number_of_id_entries,
        entries: root
            .entries
            .into_iter()
            .map(|entry| PeResourceDirectoryEntry {
                entry_rva: entry.entry_rva,
                entry_file_offset: entry.entry_file_offset,
                raw_name_or_id: entry.raw_name_or_id,
                raw_data_or_subdirectory: entry.raw_data_or_subdirectory,
                child_directory_index: None,
            })
            .collect(),
        longest_root_path: 0,
    }
}

fn read_child(
    prepared: &PreparedPe<'_>,
    graph: &PeResourceDirectoryGraph,
    offset: u32,
    total: u32,
) -> Result<PeResourceDirectory, PeResourceDirectoryError> {
    let extent = |length| {
        if u64::from(offset) + u64::from(length) > u64::from(graph.directory_size) {
            Err(PeResourceDirectoryError::DirectoryOutsideResource {
                offset,
                length,
                directory_size: graph.directory_size,
            })
        } else {
            Ok(())
        }
    };
    extent(16)?;
    let start = RelativeVirtualAddress::new(graph.directory_rva.get() + offset);
    let resolve = |length| {
        prepared
            .resolve(start, length)
            .map_err(|cause| PeResourceDirectoryError::DirectoryRange {
                offset,
                start,
                length,
                cause,
            })
    };
    let header = resolve(16)?;
    let named = read_u16(header.bytes, 12);
    let ids = read_u16(header.bytes, 14);
    let count = u32::from(named) + u32::from(ids);
    if count > u32::from(ENTRY_LIMIT) {
        return Err(PeResourceDirectoryError::EntryLimitExceeded {
            directory_offset: offset,
            count,
            limit: ENTRY_LIMIT,
        });
    }
    if total + count > TOTAL_ENTRY_LIMIT {
        return Err(PeResourceDirectoryError::TotalEntryLimitExceeded {
            directory_offset: offset,
            count: total + count,
            limit: TOTAL_ENTRY_LIMIT,
        });
    }
    let required = 16 + 8 * count;
    extent(required)?;
    let full = resolve(required)?;
    let entries = (0..count)
        .zip(full.bytes[16..].chunks_exact(8))
        .map(|(index, bytes)| {
            let displacement = 16 + index * 8;
            PeResourceDirectoryEntry {
                entry_rva: RelativeVirtualAddress::new(start.get() + displacement),
                entry_file_offset: FileOffset::new(
                    full.file_offset.get() + u64::from(displacement),
                ),
                raw_name_or_id: read_u32(bytes, 0),
                raw_data_or_subdirectory: read_u32(bytes, 4),
                child_directory_index: None,
            }
        })
        .collect();
    Ok(PeResourceDirectory {
        directory_offset: offset,
        directory_rva: start,
        directory_file_offset: full.file_offset,
        characteristics: read_u32(full.bytes, 0),
        time_date_stamp: read_u32(full.bytes, 4),
        major_version: read_u16(full.bytes, 8),
        minor_version: read_u16(full.bytes, 10),
        number_of_named_entries: named,
        number_of_id_entries: ids,
        entries,
        longest_root_path: 0,
    })
}

fn validate_paths(
    directories: &mut [PeResourceDirectory],
    count: u16,
) -> Result<(), PeResourceDirectoryError> {
    let mut indegree = vec![0_u32; directories.len()];
    for directory in directories.iter() {
        for entry in &directory.entries {
            if let Some(child) = entry.child_directory_index {
                indegree[usize::from(child)] += 1;
            }
        }
    }
    let mut order: Vec<u16> = (0_u16..)
        .zip(&indegree)
        .filter_map(|(index, &incoming)| (incoming == 0).then_some(index))
        .collect();
    let mut cursor = 0_u16;
    while usize::from(cursor) < order.len() {
        for entry in &directories[usize::from(order[usize::from(cursor)])].entries {
            if let Some(child) = entry.child_directory_index {
                indegree[usize::from(child)] -= 1;
                if indegree[usize::from(child)] == 0 {
                    order.push(child);
                }
            }
        }
        cursor += 1;
    }
    if cursor != count {
        return Err(PeResourceDirectoryError::Cycle {
            remaining_directories: count - cursor,
        });
    }
    for parent in order {
        let parent = usize::from(parent);
        for index in 0..directories[parent].entries.len() {
            if let Some(child) = directories[parent].entries[index].child_directory_index {
                let depth = directories[parent].longest_root_path + 1;
                let child = &mut directories[usize::from(child)];
                if depth > DEPTH_LIMIT {
                    return Err(PeResourceDirectoryError::DepthLimitExceeded {
                        directory_offset: child.directory_offset,
                        depth,
                        limit: DEPTH_LIMIT,
                    });
                }
                child.longest_root_path = child.longest_root_path.max(depth);
            }
        }
    }
    Ok(())
}
