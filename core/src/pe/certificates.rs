use super::{PeDirectoryAddress, PeHeaderError, PeKind, Reader, parse_pe_headers};
use crate::FileOffset;

/// opaque table bytes; record structure, padding and signature trust are unvalidated.
///
/// ```compile_fail
/// use ring3_core::{PeCertificateTable, parse_pe_certificate_table};
///
/// fn escape() -> PeCertificateTable<'static> {
///     let bytes = vec![0; 512];
///     parse_pe_certificate_table(&bytes).unwrap().unwrap()
/// }
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeCertificateTable<'a> {
    pub kind: PeKind,
    pub file_offset: FileOffset,
    pub size: u32,
    pub raw_bytes: &'a [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeCertificateError {
    Base(PeHeaderError),
    InconsistentDirectory {
        file_offset: FileOffset,
        size: u32,
    },
    UnalignedTableOffset {
        file_offset: FileOffset,
    },
    UnalignedTableSize {
        size: u32,
    },
    TableRange {
        file_offset: FileOffset,
        size: u32,
        cause: PeHeaderError,
    },
}

/// borrows the raw certificate table using file coordinates, never rva translation.
/// section metadata and table placement relative to sections/headers are not checked.
/// a missing or zero/zero slot is absent; table contents remain opaque.
///
/// # errors
/// validates headers, slot consistency, eight-byte offset alignment, eight-byte
/// size alignment, then complete physical backing, in that order.
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn parse_pe_certificate_table(
    bytes: &[u8],
) -> Result<Option<PeCertificateTable<'_>>, PeCertificateError> {
    let headers = parse_pe_headers(bytes).map_err(PeCertificateError::Base)?;
    let Some(directory) = headers.directories[4] else {
        return Ok(None);
    };
    let PeDirectoryAddress::FileOffset(file_offset) = directory.address else {
        unreachable!("the same-input parser uses file coordinates for certificate slot 4");
    };
    let size = directory.size;
    if file_offset.get() == 0 && size == 0 {
        return Ok(None);
    }
    if file_offset.get() == 0 || size == 0 {
        return Err(PeCertificateError::InconsistentDirectory { file_offset, size });
    }
    if !file_offset.get().is_multiple_of(8) {
        return Err(PeCertificateError::UnalignedTableOffset { file_offset });
    }
    if !size.is_multiple_of(8) {
        return Err(PeCertificateError::UnalignedTableSize { size });
    }
    let (raw_bytes, _) = Reader { bytes }
        .read(file_offset, u64::from(size))
        .map_err(|cause| PeCertificateError::TableRange {
            file_offset,
            size,
            cause,
        })?;
    Ok(Some(PeCertificateTable {
        kind: headers.prefix.kind,
        file_offset,
        size,
        raw_bytes,
    }))
}
