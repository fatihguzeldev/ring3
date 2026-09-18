use super::{PeTlsDirectory, PeTlsDirectoryError, parse_prepared_directory};
use crate::pe::optional::{read_u32, read_u64};
use crate::pe::rva::PreparedPe;
use crate::pe::{PeKind, PeRvaError};
use crate::{FileOffset, RelativeVirtualAddress};

/// limits retained nonzero pointers, excluding the required zero terminator.
/// this is not an input-size, allocation-byte or complete-work budget.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeTlsCallbackLimits {
    pub max_callbacks: u16,
}

/// raw callback va and its table slot; the target is neither resolved nor invoked.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeTlsCallbackEntry {
    pub table_index: u32,
    pub slot_rva: RelativeVirtualAddress,
    pub slot_file_offset: FileOffset,
    pub raw_va: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeTlsCallbackTable {
    pub table_rva: RelativeVirtualAddress,
    pub table_file_offset: FileOffset,
    pub entries: Vec<PeTlsCallbackEntry>,
    pub terminator_rva: RelativeVirtualAddress,
    pub terminator_file_offset: FileOffset,
}

/// owned same-input metadata; a null callback field has no referenced table.
/// a present empty table retains the location of its first zero pointer.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeTlsCallbacks {
    pub directory: PeTlsDirectory,
    pub image_base: u64,
    pub table: Option<PeTlsCallbackTable>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeTlsCallbackError {
    Directory(PeTlsDirectoryError),
    CallbackAddressBelowImageBase {
        address: u64,
        image_base: u64,
    },
    CallbackRvaOverflow {
        address: u64,
        image_base: u64,
    },
    TableRange {
        table_index: u32,
        table_rva: RelativeVirtualAddress,
        length: u32,
        cause: PeRvaError,
    },
    CallbackLimitExceeded {
        index: u32,
        limit: u16,
    },
}

/// reads an owned, bounded tls callback pointer list using the preferred image base.
/// absent directories, null callback fields and present empty tables stay distinct.
/// null fields are preserved without a table read; this is not loader acceptance.
/// raw targets, duplicates and order are retained without invocation or validation.
/// each complete prefix through the first zero must have one file-backed owner.
/// the callback cap permits one extra slot to prove termination; no cap-based
/// preallocation, input mutation, relocation or tls runtime action occurs.
///
/// # errors
/// preserves fixed-directory errors first, then checks table va subtraction and
/// rva fit. each prefix's backing precedes decode, zero termination and count
/// refusal, so unreadable extra slots remain range errors at the callback cap.
///
/// ```
/// use ring3_core::{PeTlsCallbackError, PeTlsCallbackLimits, parse_pe_tls_callbacks};
/// assert!(matches!(parse_pe_tls_callbacks(b"invalid", PeTlsCallbackLimits { max_callbacks: 0 }),
///     Err(PeTlsCallbackError::Directory(_))));
/// ```
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn parse_pe_tls_callbacks(
    bytes: &[u8],
    limits: PeTlsCallbackLimits,
) -> Result<Option<PeTlsCallbacks>, PeTlsCallbackError> {
    let prepared = PreparedPe::new(bytes)
        .map_err(|cause| PeTlsCallbackError::Directory(PeTlsDirectoryError::Base(cause)))?;
    let Some(directory) =
        parse_prepared_directory(&prepared).map_err(PeTlsCallbackError::Directory)?
    else {
        return Ok(None);
    };
    let image_base = prepared.headers().optional.image_base;
    let address = directory.address_of_callbacks;
    if address == 0 {
        return Ok(Some(PeTlsCallbacks {
            directory,
            image_base,
            table: None,
        }));
    }
    let delta = address.checked_sub(image_base).ok_or(
        PeTlsCallbackError::CallbackAddressBelowImageBase {
            address,
            image_base,
        },
    )?;
    let table_rva = RelativeVirtualAddress::new(u32::try_from(delta).map_err(|_| {
        PeTlsCallbackError::CallbackRvaOverflow {
            address,
            image_base,
        }
    })?);
    let width = match directory.kind {
        PeKind::Pe32 => 4_u8,
        PeKind::Pe32Plus => 8,
    };
    let mut entries = Vec::new();
    for index in 0..=u32::from(limits.max_callbacks) {
        let length = (index + 1) * u32::from(width);
        let prefix = prepared.resolve(table_rva, length).map_err(|cause| {
            PeTlsCallbackError::TableRange {
                table_index: index,
                table_rva,
                length,
                cause,
            }
        })?;
        let offset = index * u32::from(width);
        let offset_in_prefix = prefix.bytes.len() - usize::from(width);
        let raw_va = match directory.kind {
            PeKind::Pe32 => u64::from(read_u32(prefix.bytes, offset_in_prefix)),
            PeKind::Pe32Plus => read_u64(prefix.bytes, offset_in_prefix),
        };
        let slot_rva = RelativeVirtualAddress::new(table_rva.get() + offset);
        let slot_file_offset = FileOffset::new(prefix.file_offset.get() + u64::from(offset));
        if raw_va == 0 {
            return Ok(Some(PeTlsCallbacks {
                directory,
                image_base,
                table: Some(PeTlsCallbackTable {
                    table_rva,
                    table_file_offset: prefix.file_offset,
                    entries,
                    terminator_rva: slot_rva,
                    terminator_file_offset: slot_file_offset,
                }),
            }));
        }
        if index == u32::from(limits.max_callbacks) {
            return Err(PeTlsCallbackError::CallbackLimitExceeded {
                index,
                limit: limits.max_callbacks,
            });
        }
        entries.push(PeTlsCallbackEntry {
            table_index: index,
            slot_rva,
            slot_file_offset,
            raw_va,
        });
    }
    unreachable!("the inclusive final slot returns either a terminator or count refusal")
}
