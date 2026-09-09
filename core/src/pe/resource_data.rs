use std::collections::BTreeMap;

use super::optional::read_u32;
use super::resource_directories::parse_prepared_resource_directories;
use super::rva::PreparedPe;
use super::{PeResourceDirectoryError, PeResourceDirectoryGraph, PeResourceRootError, PeRvaError};
use crate::{FileOffset, RelativeVirtualAddress};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeResourceDataEntry {
    pub data_entry_offset: u32,
    pub data_entry_rva: RelativeVirtualAddress,
    pub data_entry_file_offset: FileOffset,
    pub payload_rva: RelativeVirtualAddress,
    pub payload_size: u32,
    pub code_page: u32,
    pub reserved: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeResourceDataReference {
    pub directory_index: u16,
    pub entry_index: u16,
    pub data_entry_index: u16,
}

/// one raw record per unique offset and one reference per leaf entry occurrence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeResourceDataEntryTable {
    pub directory_graph: PeResourceDirectoryGraph,
    pub data_entries: Vec<PeResourceDataEntry>,
    pub references: Vec<PeResourceDataReference>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeResourceDataEntryError {
    Graph(PeResourceDirectoryError),
    DataEntryOutsideResource {
        directory_index: u16,
        entry_index: u16,
        offset: u32,
        length: u32,
        directory_size: u32,
    },
    DataEntryRange {
        directory_index: u16,
        entry_index: u16,
        offset: u32,
        start: RelativeVirtualAddress,
        length: u32,
        cause: PeRvaError,
    },
}

/// reads fixed resource data-entry metadata through one prepared input.
/// the supported directory graph bounds references and unique records to at most
/// 4096 each. first leaf occurrence determines record order; shared paths are not
/// expanded. names and payloads remain unread, including invalid payload ranges
/// and nonzero reserved fields. zero or overlapping record offsets remain raw.
///
/// # errors
/// validates the entire supported directory graph first. for each unseen leaf,
/// checks its 16-byte resource-relative extent, then conservative full backing.
/// data errors identify the first referring directory and raw entry index.
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn parse_pe_resource_data_entries(
    bytes: &[u8],
) -> Result<Option<PeResourceDataEntryTable>, PeResourceDataEntryError> {
    let prepared = PreparedPe::new(bytes).map_err(|cause| {
        PeResourceDataEntryError::Graph(PeResourceDirectoryError::Root(PeResourceRootError::Base(
            cause,
        )))
    })?;
    let Some(directory_graph) =
        parse_prepared_resource_directories(&prepared).map_err(PeResourceDataEntryError::Graph)?
    else {
        return Ok(None);
    };
    let mut data_entries = Vec::new();
    let mut references = Vec::new();
    let mut identities = BTreeMap::new();
    let mut record_count = 0_u16;
    for (directory_index, directory) in (0_u16..).zip(&directory_graph.directories) {
        for (entry_index, entry) in (0_u16..).zip(&directory.entries) {
            if entry.child_directory_index.is_some() {
                continue;
            }
            let offset = entry.raw_data_or_subdirectory;
            let data_entry_index = if let Some(&known) = identities.get(&offset) {
                known
            } else {
                if u64::from(offset) + 16 > u64::from(directory_graph.directory_size) {
                    return Err(PeResourceDataEntryError::DataEntryOutsideResource {
                        directory_index,
                        entry_index,
                        offset,
                        length: 16,
                        directory_size: directory_graph.directory_size,
                    });
                }
                let start =
                    RelativeVirtualAddress::new(directory_graph.directory_rva.get() + offset);
                let record = prepared.resolve(start, 16).map_err(|cause| {
                    PeResourceDataEntryError::DataEntryRange {
                        directory_index,
                        entry_index,
                        offset,
                        start,
                        length: 16,
                        cause,
                    }
                })?;
                data_entries.push(PeResourceDataEntry {
                    data_entry_offset: offset,
                    data_entry_rva: start,
                    data_entry_file_offset: record.file_offset,
                    payload_rva: RelativeVirtualAddress::new(read_u32(record.bytes, 0)),
                    payload_size: read_u32(record.bytes, 4),
                    code_page: read_u32(record.bytes, 8),
                    reserved: read_u32(record.bytes, 12),
                });
                let index = record_count;
                identities.insert(offset, index);
                record_count += 1;
                index
            };
            references.push(PeResourceDataReference {
                directory_index,
                entry_index,
                data_entry_index,
            });
        }
    }
    Ok(Some(PeResourceDataEntryTable {
        directory_graph,
        data_entries,
        references,
    }))
}
