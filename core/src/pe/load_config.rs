use super::optional::{read_u16, read_u32};
use super::rva::PreparedPe;
use super::{PeDirectoryAddress, PeKind, PeRvaError};
use crate::{FileOffset, RelativeVirtualAddress};

const PREFIX_SIZE: u32 = 24;

/// common metadata for size-bearing structures; neither declared tail is read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeLoadConfigPrefix {
    pub kind: PeKind,
    pub directory_rva: RelativeVirtualAddress,
    pub directory_file_offset: FileOffset,
    pub directory_size: u32,
    pub structure_size: u32,
    pub time_date_stamp: u32,
    pub major_version: u16,
    pub minor_version: u16,
    pub global_flags_clear: u32,
    pub global_flags_set: u32,
    pub critical_section_default_timeout: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeLoadConfigError {
    Base(PeRvaError),
    InconsistentDirectory {
        rva: RelativeVirtualAddress,
        size: u32,
    },
    DirectoryRangeOverflow {
        rva: RelativeVirtualAddress,
        size: u32,
    },
    TruncatedDirectory {
        rva: RelativeVirtualAddress,
        size: u32,
        required: u32,
    },
    PrefixRange {
        start: RelativeVirtualAddress,
        length: u32,
        cause: PeRvaError,
    },
    UnsupportedStructureSize {
        structure_size: u32,
        required: u32,
    },
}

/// reads the shared 24-byte load-config prefix with an embedded size of at least 24.
/// declared sizes remain independent; unknown flags and versions stay unclassified.
/// no later pointer field, security table, or legacy characteristics form is read.
///
/// # errors
/// validates the base file, slot consistency, directory coordinate end and minimum,
/// conservative physical prefix, then the embedded size minimum, in that order.
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn parse_pe_load_config_prefix(
    bytes: &[u8],
) -> Result<Option<PeLoadConfigPrefix>, PeLoadConfigError> {
    let prepared = PreparedPe::new(bytes).map_err(PeLoadConfigError::Base)?;
    let Some(directory) = prepared.headers().directories[10] else {
        return Ok(None);
    };
    let PeDirectoryAddress::Rva(rva) = directory.address else {
        unreachable!("the same-input parser uses rva coordinates for load-config slot 10");
    };
    let size = directory.size;
    if rva.get() == 0 && size == 0 {
        return Ok(None);
    }
    if rva.get() == 0 || size == 0 {
        return Err(PeLoadConfigError::InconsistentDirectory { rva, size });
    }
    if u64::from(rva.get()) + u64::from(size) > 1_u64 << 32 {
        return Err(PeLoadConfigError::DirectoryRangeOverflow { rva, size });
    }
    if size < PREFIX_SIZE {
        return Err(PeLoadConfigError::TruncatedDirectory {
            rva,
            size,
            required: PREFIX_SIZE,
        });
    }
    let prefix =
        prepared
            .resolve(rva, PREFIX_SIZE)
            .map_err(|cause| PeLoadConfigError::PrefixRange {
                start: rva,
                length: PREFIX_SIZE,
                cause,
            })?;
    let record = prefix.bytes;
    let structure_size = read_u32(record, 0);
    if structure_size < PREFIX_SIZE {
        return Err(PeLoadConfigError::UnsupportedStructureSize {
            structure_size,
            required: PREFIX_SIZE,
        });
    }
    Ok(Some(PeLoadConfigPrefix {
        kind: prepared.headers().prefix.kind,
        directory_rva: rva,
        directory_file_offset: prefix.file_offset,
        directory_size: size,
        structure_size,
        time_date_stamp: read_u32(record, 4),
        major_version: read_u16(record, 8),
        minor_version: read_u16(record, 10),
        global_flags_clear: read_u32(record, 12),
        global_flags_set: read_u32(record, 16),
        critical_section_default_timeout: read_u32(record, 20),
    }))
}
