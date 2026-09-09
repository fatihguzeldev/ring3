use super::optional::{read_u16, read_u32};
use super::rva::PreparedPe;
use super::{PeDirectoryAddress, PeKind, PeRvaError};
use crate::{FileOffset, RelativeVirtualAddress};

const HEADER_SIZE: u32 = 16;
const ENTRY_SIZE: u32 = 8;
const ENTRY_LIMIT: u16 = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeResourceRootEntry {
    pub entry_rva: RelativeVirtualAddress,
    pub entry_file_offset: FileOffset,
    pub raw_name_or_id: u32,
    pub raw_data_or_subdirectory: u32,
}

/// owned root metadata; entry words remain raw and no resource target is read.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeResourceRoot {
    pub kind: PeKind,
    pub directory_rva: RelativeVirtualAddress,
    pub directory_file_offset: FileOffset,
    pub directory_size: u32,
    pub characteristics: u32,
    pub time_date_stamp: u32,
    pub major_version: u16,
    pub minor_version: u16,
    pub number_of_named_entries: u16,
    pub number_of_id_entries: u16,
    pub entries: Vec<PeResourceRootEntry>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeResourceRootError {
    Base(PeRvaError),
    InconsistentDirectory {
        rva: RelativeVirtualAddress,
        size: u32,
    },
    DirectoryRangeOverflow {
        rva: RelativeVirtualAddress,
        size: u32,
    },
    TruncatedRootHeader {
        rva: RelativeVirtualAddress,
        size: u32,
        required: u32,
    },
    RootRange {
        start: RelativeVirtualAddress,
        length: u32,
        cause: PeRvaError,
    },
    EntryLimitExceeded {
        count: u32,
        limit: u16,
    },
    TruncatedRootEntries {
        size: u32,
        required: u32,
    },
}

/// reads a resource root header and at most 256 ordered raw entries.
/// names, subdirectories, data entries, payloads and the remaining tail are unread.
///
/// # errors
/// validates the base file, slot consistency, directory coordinate end and header
/// minimum, header backing, count limit, declared entry extent, then full backing.
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn parse_pe_resource_root(bytes: &[u8]) -> Result<Option<PeResourceRoot>, PeResourceRootError> {
    let prepared = PreparedPe::new(bytes).map_err(PeResourceRootError::Base)?;
    parse_prepared_resource_root(&prepared)
}

pub(super) fn parse_prepared_resource_root(
    prepared: &PreparedPe<'_>,
) -> Result<Option<PeResourceRoot>, PeResourceRootError> {
    let Some(directory) = prepared.headers().directories[2] else {
        return Ok(None);
    };
    let PeDirectoryAddress::Rva(rva) = directory.address else {
        unreachable!("the same-input parser uses rva coordinates for resource slot 2");
    };
    let size = directory.size;
    if rva.get() == 0 && size == 0 {
        return Ok(None);
    }
    if rva.get() == 0 || size == 0 {
        return Err(PeResourceRootError::InconsistentDirectory { rva, size });
    }
    if u64::from(rva.get()) + u64::from(size) > 1_u64 << 32 {
        return Err(PeResourceRootError::DirectoryRangeOverflow { rva, size });
    }
    if size < HEADER_SIZE {
        return Err(PeResourceRootError::TruncatedRootHeader {
            rva,
            size,
            required: HEADER_SIZE,
        });
    }
    let resolve = |length| {
        prepared
            .resolve(rva, length)
            .map_err(|cause| PeResourceRootError::RootRange {
                start: rva,
                length,
                cause,
            })
    };
    let header = resolve(HEADER_SIZE)?;
    let number_of_named_entries = read_u16(header.bytes, 12);
    let number_of_id_entries = read_u16(header.bytes, 14);
    let count = u32::from(number_of_named_entries) + u32::from(number_of_id_entries);
    if count > u32::from(ENTRY_LIMIT) {
        return Err(PeResourceRootError::EntryLimitExceeded {
            count,
            limit: ENTRY_LIMIT,
        });
    }
    let required = HEADER_SIZE + count * ENTRY_SIZE;
    if required > size {
        return Err(PeResourceRootError::TruncatedRootEntries { size, required });
    }
    let root = resolve(required)?;
    let entries = (0..count)
        .zip(root.bytes[HEADER_SIZE as usize..].chunks_exact(ENTRY_SIZE as usize))
        .map(|(index, record)| {
            let displacement = HEADER_SIZE + index * ENTRY_SIZE;
            PeResourceRootEntry {
                entry_rva: RelativeVirtualAddress::new(rva.get() + displacement),
                entry_file_offset: FileOffset::new(
                    root.file_offset.get() + u64::from(displacement),
                ),
                raw_name_or_id: read_u32(record, 0),
                raw_data_or_subdirectory: read_u32(record, 4),
            }
        })
        .collect();
    Ok(Some(PeResourceRoot {
        kind: prepared.headers().prefix.kind,
        directory_rva: rva,
        directory_file_offset: root.file_offset,
        directory_size: size,
        characteristics: read_u32(root.bytes, 0),
        time_date_stamp: read_u32(root.bytes, 4),
        major_version: read_u16(root.bytes, 8),
        minor_version: read_u16(root.bytes, 10),
        number_of_named_entries,
        number_of_id_entries,
        entries,
    }))
}
