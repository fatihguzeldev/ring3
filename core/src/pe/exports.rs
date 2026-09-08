use super::optional::{read_u16, read_u32};
use super::rva::PreparedPe;
use super::{PeDirectoryAddress, PeRvaError};
use crate::{FileOffset, RelativeVirtualAddress};

const DIRECTORY_LENGTH: u32 = 40;

/// fixed export metadata; counts and target rvas are retained without traversal.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeExportDirectory {
    pub directory_rva: RelativeVirtualAddress,
    pub directory_file_offset: FileOffset,
    pub directory_size: u32,
    pub flags: u32,
    pub time_date_stamp: u32,
    pub major_version: u16,
    pub minor_version: u16,
    pub name_rva: RelativeVirtualAddress,
    pub ordinal_base: u32,
    pub address_table_entries: u32,
    pub number_of_name_pointers: u32,
    pub export_address_table_rva: RelativeVirtualAddress,
    pub name_pointer_rva: RelativeVirtualAddress,
    pub ordinal_table_rva: RelativeVirtualAddress,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeExportDirectoryError {
    Base(PeRvaError),
    InconsistentExportDirectory {
        rva: RelativeVirtualAddress,
        size: u32,
    },
    ExportDirectoryRangeOverflow {
        rva: RelativeVirtualAddress,
        size: u32,
    },
    TruncatedExportDirectory {
        rva: RelativeVirtualAddress,
        size: u32,
        required: u32,
    },
    DirectoryRange {
        start: RelativeVirtualAddress,
        length: u32,
        cause: PeRvaError,
    },
}

/// reads one 40-byte directory; the declared tail and all targets remain unread.
/// a missing or zero/zero slot is absent; a present zero record is retained.
///
/// # errors
/// validates the base file, slot consistency, declared coordinate end, minimum
/// size, and conservative physical prefix, in that order.
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn parse_pe_export_directory(
    bytes: &[u8],
) -> Result<Option<PeExportDirectory>, PeExportDirectoryError> {
    let prepared = PreparedPe::new(bytes).map_err(PeExportDirectoryError::Base)?;
    parse_prepared_export_directory(&prepared)
}

pub(super) fn parse_prepared_export_directory(
    prepared: &PreparedPe<'_>,
) -> Result<Option<PeExportDirectory>, PeExportDirectoryError> {
    let Some(directory) = prepared.headers().directories[0] else {
        return Ok(None);
    };
    let PeDirectoryAddress::Rva(rva) = directory.address else {
        unreachable!("the same-input parser uses rva coordinates for export slot 0");
    };
    let size = directory.size;
    if rva.get() == 0 && size == 0 {
        return Ok(None);
    }
    if rva.get() == 0 || size == 0 {
        return Err(PeExportDirectoryError::InconsistentExportDirectory { rva, size });
    }
    if u64::from(rva.get()) + u64::from(size) > 1_u64 << 32 {
        return Err(PeExportDirectoryError::ExportDirectoryRangeOverflow { rva, size });
    }
    if size < DIRECTORY_LENGTH {
        return Err(PeExportDirectoryError::TruncatedExportDirectory {
            rva,
            size,
            required: DIRECTORY_LENGTH,
        });
    }
    let prefix = prepared.resolve(rva, DIRECTORY_LENGTH).map_err(|cause| {
        PeExportDirectoryError::DirectoryRange {
            start: rva,
            length: DIRECTORY_LENGTH,
            cause,
        }
    })?;
    let record = prefix.bytes;
    Ok(Some(PeExportDirectory {
        directory_rva: rva,
        directory_file_offset: prefix.file_offset,
        directory_size: size,
        flags: read_u32(record, 0),
        time_date_stamp: read_u32(record, 4),
        major_version: read_u16(record, 8),
        minor_version: read_u16(record, 10),
        name_rva: RelativeVirtualAddress::new(read_u32(record, 12)),
        ordinal_base: read_u32(record, 16),
        address_table_entries: read_u32(record, 20),
        number_of_name_pointers: read_u32(record, 24),
        export_address_table_rva: RelativeVirtualAddress::new(read_u32(record, 28)),
        name_pointer_rva: RelativeVirtualAddress::new(read_u32(record, 32)),
        ordinal_table_rva: RelativeVirtualAddress::new(read_u32(record, 36)),
    }))
}
