use super::optional::{read_u16, read_u32};
use super::rva::PreparedPe;
use super::{PeDirectoryAddress, PeKind, PeRvaError};
use crate::{FileOffset, RelativeVirtualAddress};

const HEADER_SIZE: u32 = 72;

/// raw coordinates only; the target range is not read or validated.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeClrDataDirectory {
    pub rva: RelativeVirtualAddress,
    pub size: u32,
}

/// fixed clr header metadata; neither declared tail nor nested targets are read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeClrHeader {
    pub kind: PeKind,
    pub directory_rva: RelativeVirtualAddress,
    pub directory_file_offset: FileOffset,
    pub directory_size: u32,
    pub header_size: u32,
    pub major_runtime_version: u16,
    pub minor_runtime_version: u16,
    pub flags: u32,
    /// token when flag 0x10 is clear, native rva when set; validity is not checked.
    pub raw_entry_point: u32,
    pub metadata: PeClrDataDirectory,
    pub resources: PeClrDataDirectory,
    pub strong_name_signature: PeClrDataDirectory,
    pub code_manager_table: PeClrDataDirectory,
    pub v_table_fixups: PeClrDataDirectory,
    pub export_address_table_jumps: PeClrDataDirectory,
    pub managed_native_header: PeClrDataDirectory,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeClrError {
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
    UnsupportedHeaderSize {
        header_size: u32,
        required: u32,
    },
}

fn read_directory(bytes: &[u8], offset: usize) -> PeClrDataDirectory {
    PeClrDataDirectory {
        rva: RelativeVirtualAddress::new(read_u32(bytes, offset)),
        size: read_u32(bytes, offset + 4),
    }
}

/// reads slot 14's fixed 72-byte header with an embedded size of at least 72.
/// directory size and embedded size remain independent; neither tail is read.
/// raw flags, versions, entry word and nested coordinates are not clr acceptance.
///
/// # errors
/// validates the base file, slot consistency, directory coordinate end and minimum,
/// conservative physical prefix, then the embedded size minimum, in that order.
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn parse_pe_clr_header(bytes: &[u8]) -> Result<Option<PeClrHeader>, PeClrError> {
    let prepared = PreparedPe::new(bytes).map_err(PeClrError::Base)?;
    let Some(directory) = prepared.headers().directories[14] else {
        return Ok(None);
    };
    let PeDirectoryAddress::Rva(rva) = directory.address else {
        unreachable!("the same-input parser uses rva coordinates for clr slot 14");
    };
    let size = directory.size;
    if rva.get() == 0 && size == 0 {
        return Ok(None);
    }
    if rva.get() == 0 || size == 0 {
        return Err(PeClrError::InconsistentDirectory { rva, size });
    }
    if u64::from(rva.get()) + u64::from(size) > 1_u64 << 32 {
        return Err(PeClrError::DirectoryRangeOverflow { rva, size });
    }
    if size < HEADER_SIZE {
        return Err(PeClrError::TruncatedDirectory {
            rva,
            size,
            required: HEADER_SIZE,
        });
    }
    let prefix = prepared
        .resolve(rva, HEADER_SIZE)
        .map_err(|cause| PeClrError::PrefixRange {
            start: rva,
            length: HEADER_SIZE,
            cause,
        })?;
    let record = prefix.bytes;
    let header_size = read_u32(record, 0);
    if header_size < HEADER_SIZE {
        return Err(PeClrError::UnsupportedHeaderSize {
            header_size,
            required: HEADER_SIZE,
        });
    }
    Ok(Some(PeClrHeader {
        kind: prepared.headers().prefix.kind,
        directory_rva: rva,
        directory_file_offset: prefix.file_offset,
        directory_size: size,
        header_size,
        major_runtime_version: read_u16(record, 4),
        minor_runtime_version: read_u16(record, 6),
        flags: read_u32(record, 16),
        raw_entry_point: read_u32(record, 20),
        metadata: read_directory(record, 8),
        resources: read_directory(record, 24),
        strong_name_signature: read_directory(record, 32),
        code_manager_table: read_directory(record, 40),
        v_table_fixups: read_directory(record, 48),
        export_address_table_jumps: read_directory(record, 56),
        managed_native_header: read_directory(record, 64),
    }))
}
