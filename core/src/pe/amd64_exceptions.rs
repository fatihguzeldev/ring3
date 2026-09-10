use super::optional::read_u32;
use super::rva::PreparedPe;
use super::{PeDirectoryAddress, PeKind, PeRvaError};
use crate::{FileOffset, RelativeVirtualAddress};

const ENTRY_SIZE: u32 = 12;
const MAX_ENTRIES: u16 = 4096;

/// raw function coordinates; function and unwind targets remain unread.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeAmd64ExceptionEntry {
    pub table_index: u32,
    pub entry_rva: RelativeVirtualAddress,
    pub entry_file_offset: FileOffset,
    pub begin_rva: RelativeVirtualAddress,
    pub end_rva: RelativeVirtualAddress,
    pub unwind_info_rva: RelativeVirtualAddress,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeAmd64ExceptionTable {
    pub directory_rva: RelativeVirtualAddress,
    pub directory_file_offset: FileOffset,
    pub directory_size: u32,
    pub entries: Vec<PeAmd64ExceptionEntry>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeAmd64ExceptionError {
    Base(PeRvaError),
    InconsistentDirectory {
        rva: RelativeVirtualAddress,
        size: u32,
    },
    DirectoryRangeOverflow {
        rva: RelativeVirtualAddress,
        size: u32,
    },
    UnsupportedImage {
        machine: u16,
        kind: PeKind,
    },
    UnalignedDirectory {
        rva: RelativeVirtualAddress,
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

/// reads at most 4096 raw amd64 exception function records from directory slot 3.
/// a present table requires pe32+ with machine 0x8664 and a four-byte aligned rva.
/// an absent slot is accepted for any otherwise accepted image.
///
/// records retain file order, zero triples, duplicates, overlaps and empty or
/// reversed function ranges. no function or unwind target is followed or checked.
/// the result owns scalar metadata; the entry cap is not an input or memory budget.
///
/// # errors
/// validates the complete base file, slot consistency, coordinate end, image kind,
/// rva alignment, record size, entry cap and whole file-backed range, in that order.
///
/// ```
/// use ring3_core::{PeAmd64ExceptionError, parse_pe_amd64_exception_functions};
/// assert!(matches!(parse_pe_amd64_exception_functions(b"invalid"),
///     Err(PeAmd64ExceptionError::Base(_))));
/// ```
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn parse_pe_amd64_exception_functions(
    bytes: &[u8],
) -> Result<Option<PeAmd64ExceptionTable>, PeAmd64ExceptionError> {
    let prepared = PreparedPe::new(bytes).map_err(PeAmd64ExceptionError::Base)?;
    let Some(directory) = prepared.headers().directories[3] else {
        return Ok(None);
    };
    let PeDirectoryAddress::Rva(rva) = directory.address else {
        unreachable!("the same-input parser uses rva coordinates for exception slot 3");
    };
    let size = directory.size;
    if rva.get() == 0 && size == 0 {
        return Ok(None);
    }
    if rva.get() == 0 || size == 0 {
        return Err(PeAmd64ExceptionError::InconsistentDirectory { rva, size });
    }
    if u64::from(rva.get()) + u64::from(size) > 1_u64 << 32 {
        return Err(PeAmd64ExceptionError::DirectoryRangeOverflow { rva, size });
    }
    let prefix = prepared.headers().prefix;
    if prefix.kind != PeKind::Pe32Plus || prefix.machine != 0x8664 {
        return Err(PeAmd64ExceptionError::UnsupportedImage {
            machine: prefix.machine,
            kind: prefix.kind,
        });
    }
    if !rva.get().is_multiple_of(4) {
        return Err(PeAmd64ExceptionError::UnalignedDirectory { rva });
    }
    if !size.is_multiple_of(ENTRY_SIZE) {
        return Err(PeAmd64ExceptionError::InvalidDirectorySize { size });
    }
    let count = size / ENTRY_SIZE;
    if count > u32::from(MAX_ENTRIES) {
        return Err(PeAmd64ExceptionError::EntryLimitExceeded {
            count,
            limit: MAX_ENTRIES,
        });
    }
    let range =
        prepared
            .resolve(rva, size)
            .map_err(|cause| PeAmd64ExceptionError::DirectoryRange {
                start: rva,
                length: size,
                cause,
            })?;
    let entries = (0..count)
        .zip(range.bytes.chunks_exact(12))
        .map(|(index, record)| {
            let offset = index * ENTRY_SIZE;
            PeAmd64ExceptionEntry {
                table_index: index,
                entry_rva: RelativeVirtualAddress::new(rva.get() + offset),
                entry_file_offset: FileOffset::new(range.file_offset.get() + u64::from(offset)),
                begin_rva: RelativeVirtualAddress::new(read_u32(record, 0)),
                end_rva: RelativeVirtualAddress::new(read_u32(record, 4)),
                unwind_info_rva: RelativeVirtualAddress::new(read_u32(record, 8)),
            }
        })
        .collect();
    Ok(Some(PeAmd64ExceptionTable {
        directory_rva: rva,
        directory_file_offset: range.file_offset,
        directory_size: size,
        entries,
    }))
}
