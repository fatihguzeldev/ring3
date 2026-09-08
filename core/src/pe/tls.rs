use super::optional::{read_u32, read_u64};
use super::rva::PreparedPe;
use super::{PeDirectoryAddress, PeKind, PeRvaError};
use crate::{FileOffset, RelativeVirtualAddress};

/// fixed tls metadata; raw va fields are not rvas or mapped guest addresses.
/// pe32 addresses are zero-extended; all targets and reserved bits stay unclassified.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeTlsDirectory {
    pub kind: PeKind,
    pub directory_rva: RelativeVirtualAddress,
    pub directory_file_offset: FileOffset,
    pub directory_size: u32,
    pub start_address_of_raw_data: u64,
    pub end_address_of_raw_data: u64,
    pub address_of_index: u64,
    pub address_of_callbacks: u64,
    pub size_of_zero_fill: u32,
    pub characteristics: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeTlsDirectoryError {
    Base(PeRvaError),
    InconsistentTlsDirectory {
        rva: RelativeVirtualAddress,
        size: u32,
    },
    TlsDirectoryRangeOverflow {
        rva: RelativeVirtualAddress,
        size: u32,
    },
    TruncatedTlsDirectory {
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

/// reads the fixed 24-byte pe32 or 40-byte pe32+ tls directory.
/// the declared tail and all va targets remain unread; no image-base arithmetic,
/// tls allocation, index write or callback invocation occurs.
/// a missing or zero/zero slot is absent; a present zero record is retained.
///
/// # errors
/// validates the base file, slot consistency, declared coordinate end, width-specific
/// minimum size, and conservative physical prefix, in that order.
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn parse_pe_tls_directory(bytes: &[u8]) -> Result<Option<PeTlsDirectory>, PeTlsDirectoryError> {
    let prepared = PreparedPe::new(bytes).map_err(PeTlsDirectoryError::Base)?;
    let Some(directory) = prepared.headers().directories[9] else {
        return Ok(None);
    };
    let PeDirectoryAddress::Rva(rva) = directory.address else {
        unreachable!("the same-input parser uses rva coordinates for TLS slot 9");
    };
    let size = directory.size;
    if rva.get() == 0 && size == 0 {
        return Ok(None);
    }
    if rva.get() == 0 || size == 0 {
        return Err(PeTlsDirectoryError::InconsistentTlsDirectory { rva, size });
    }
    if u64::from(rva.get()) + u64::from(size) > 1_u64 << 32 {
        return Err(PeTlsDirectoryError::TlsDirectoryRangeOverflow { rva, size });
    }
    let kind = prepared.headers().prefix.kind;
    let required = match kind {
        PeKind::Pe32 => 24,
        PeKind::Pe32Plus => 40,
    };
    if size < required {
        return Err(PeTlsDirectoryError::TruncatedTlsDirectory {
            rva,
            size,
            required,
        });
    }
    let prefix =
        prepared
            .resolve(rva, required)
            .map_err(|cause| PeTlsDirectoryError::DirectoryRange {
                start: rva,
                length: required,
                cause,
            })?;
    let record = prefix.bytes;
    let (addresses, tail) = match kind {
        PeKind::Pe32 => (
            [0, 4, 8, 12].map(|offset| u64::from(read_u32(record, offset))),
            16,
        ),
        PeKind::Pe32Plus => ([0, 8, 16, 24].map(|offset| read_u64(record, offset)), 32),
    };
    Ok(Some(PeTlsDirectory {
        kind,
        directory_rva: rva,
        directory_file_offset: prefix.file_offset,
        directory_size: size,
        start_address_of_raw_data: addresses[0],
        end_address_of_raw_data: addresses[1],
        address_of_index: addresses[2],
        address_of_callbacks: addresses[3],
        size_of_zero_fill: read_u32(record, tail),
        characteristics: read_u32(record, tail + 4),
    }))
}
