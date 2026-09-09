use super::optional::{read_u16, read_u32};
use super::rva::PreparedPe;
use super::{PeDirectoryAddress, PeKind, PeRvaError};
use crate::{FileOffset, RelativeVirtualAddress};

const ENTRY_SIZE: u32 = 28;
const MAX_ENTRIES: u16 = 256;

/// raw metadata; payload coordinates are independent and their targets are unread.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeDebugDirectoryEntry {
    pub entry_rva: RelativeVirtualAddress,
    pub entry_file_offset: FileOffset,
    pub characteristics: u32,
    pub time_date_stamp: u32,
    pub major_version: u16,
    pub minor_version: u16,
    pub debug_type: u32,
    pub size_of_data: u32,
    pub address_of_raw_data: RelativeVirtualAddress,
    pub pointer_to_raw_data: FileOffset,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeDebugDirectoryTable {
    pub kind: PeKind,
    pub directory_rva: RelativeVirtualAddress,
    pub directory_file_offset: FileOffset,
    pub directory_size: u32,
    pub entries: Vec<PeDebugDirectoryEntry>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeDebugDirectoryError {
    Base(PeRvaError),
    InconsistentDirectory {
        rva: RelativeVirtualAddress,
        size: u32,
    },
    DirectoryRangeOverflow {
        rva: RelativeVirtualAddress,
        size: u32,
    },
    InvalidDirectorySize {
        size: u32,
    },
    EntryLimitExceeded {
        count: u32,
        limit: u16,
    },
    DirectoryRange {
        start: RelativeVirtualAddress,
        length: u32,
        cause: PeRvaError,
    },
}

/// reads at most 256 fixed debug records, including zero and unknown records.
/// the result owns scalar metadata; no debug payload or path is read.
///
/// # errors
/// validates the base file, slot consistency, declared coordinate end, record size,
/// entry count, and the complete conservative file-backed table, in that order.
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn parse_pe_debug_directory(
    bytes: &[u8],
) -> Result<Option<PeDebugDirectoryTable>, PeDebugDirectoryError> {
    let prepared = PreparedPe::new(bytes).map_err(PeDebugDirectoryError::Base)?;
    let Some(directory) = prepared.headers().directories[6] else {
        return Ok(None);
    };
    let PeDirectoryAddress::Rva(rva) = directory.address else {
        unreachable!("the same-input parser uses rva coordinates for debug slot 6");
    };
    let size = directory.size;
    if rva.get() == 0 && size == 0 {
        return Ok(None);
    }
    if rva.get() == 0 || size == 0 {
        return Err(PeDebugDirectoryError::InconsistentDirectory { rva, size });
    }
    if u64::from(rva.get()) + u64::from(size) > 1_u64 << 32 {
        return Err(PeDebugDirectoryError::DirectoryRangeOverflow { rva, size });
    }
    if !size.is_multiple_of(ENTRY_SIZE) {
        return Err(PeDebugDirectoryError::InvalidDirectorySize { size });
    }
    let count = size / ENTRY_SIZE;
    if count > u32::from(MAX_ENTRIES) {
        return Err(PeDebugDirectoryError::EntryLimitExceeded {
            count,
            limit: MAX_ENTRIES,
        });
    }
    let range =
        prepared
            .resolve(rva, size)
            .map_err(|cause| PeDebugDirectoryError::DirectoryRange {
                start: rva,
                length: size,
                cause,
            })?;
    let entries = (0..count)
        .zip(range.bytes.chunks_exact(28))
        .map(|(index, record)| {
            let offset = index * ENTRY_SIZE;
            PeDebugDirectoryEntry {
                entry_rva: RelativeVirtualAddress::new(rva.get() + offset),
                entry_file_offset: FileOffset::new(range.file_offset.get() + u64::from(offset)),
                characteristics: read_u32(record, 0),
                time_date_stamp: read_u32(record, 4),
                major_version: read_u16(record, 8),
                minor_version: read_u16(record, 10),
                debug_type: read_u32(record, 12),
                size_of_data: read_u32(record, 16),
                address_of_raw_data: RelativeVirtualAddress::new(read_u32(record, 20)),
                pointer_to_raw_data: FileOffset::new(u64::from(read_u32(record, 24))),
            }
        })
        .collect();
    Ok(Some(PeDebugDirectoryTable {
        kind: prepared.headers().prefix.kind,
        directory_rva: rva,
        directory_file_offset: range.file_offset,
        directory_size: size,
        entries,
    }))
}
