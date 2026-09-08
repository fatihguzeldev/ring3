use super::exports::parse_prepared_export_directory;
use super::optional::read_u32;
use super::rva::PreparedPe;
use super::{PeExportDirectory, PeExportDirectoryError, PeRvaError};
use crate::{FileOffset, RelativeVirtualAddress};

const ENTRY_LIMIT: u32 = 4096;
const FORWARDER_LENGTH_LIMIT: u32 = 1024;
const FORWARDER_SCAN_BUDGET: u32 = 65_536;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeExportTarget<'a> {
    Empty,
    Rva(RelativeVirtualAddress),
    Forwarder {
        rva: RelativeVirtualAddress,
        text: &'a str,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeExportAddressEntry<'a> {
    pub table_index: u32,
    pub ordinal: u32,
    pub entry_rva: RelativeVirtualAddress,
    pub entry_file_offset: FileOffset,
    pub target: PeExportTarget<'a>,
}

/// static address entries and raw forwarder text borrowed from the input.
///
/// ```compile_fail
/// use ring3_core::{PeExportAddressTable, parse_pe_export_addresses};
///
/// fn escape() -> Option<PeExportAddressTable<'static>> {
///     let bytes = vec![0; 64];
///     parse_pe_export_addresses(&bytes).unwrap()
/// }
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeExportAddressTable<'a> {
    pub directory: PeExportDirectory,
    pub entries: Vec<PeExportAddressEntry<'a>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeExportAddressError {
    Directory(PeExportDirectoryError),
    EntryLimitExceeded {
        count: u32,
        limit: u32,
    },
    AddressTableUnavailable {
        count: u32,
    },
    AddressTableRange {
        start: RelativeVirtualAddress,
        length: u32,
        cause: PeRvaError,
    },
    OrdinalOverflow {
        ordinal_base: u32,
        entry_index: u32,
    },
    ForwarderRange {
        entry_index: u32,
        start: RelativeVirtualAddress,
        offset: u32,
        cause: PeRvaError,
    },
    EmptyForwarder {
        entry_index: u32,
        start: RelativeVirtualAddress,
    },
    NonAsciiForwarder {
        entry_index: u32,
        start: RelativeVirtualAddress,
        offset: u32,
        byte: u8,
    },
    UnterminatedForwarder {
        entry_index: u32,
        start: RelativeVirtualAddress,
        offset: u32,
        directory_end: u64,
    },
    ForwarderLengthLimitExceeded {
        entry_index: u32,
        start: RelativeVirtualAddress,
        limit: u32,
    },
    ForwarderScanBudgetExceeded {
        entry_index: u32,
        start: RelativeVirtualAddress,
        offset: u32,
        limit: u32,
    },
}

fn read_forwarder<'a>(
    prepared: &PreparedPe<'a>,
    entry_index: u32,
    start: RelativeVirtualAddress,
    directory_end: u64,
    total: &mut u32,
) -> Result<&'a str, PeExportAddressError> {
    let mut offset = 0;
    loop {
        if offset == FORWARDER_LENGTH_LIMIT {
            return Err(PeExportAddressError::ForwarderLengthLimitExceeded {
                entry_index,
                start,
                limit: FORWARDER_LENGTH_LIMIT,
            });
        }
        if *total == FORWARDER_SCAN_BUDGET {
            return Err(PeExportAddressError::ForwarderScanBudgetExceeded {
                entry_index,
                start,
                offset,
                limit: FORWARDER_SCAN_BUDGET,
            });
        }
        if u64::from(start.get()) + u64::from(offset) >= directory_end {
            return Err(PeExportAddressError::UnterminatedForwarder {
                entry_index,
                start,
                offset,
                directory_end,
            });
        }
        let prefix = prepared.resolve(start, offset + 1).map_err(|cause| {
            PeExportAddressError::ForwarderRange {
                entry_index,
                start,
                offset,
                cause,
            }
        })?;
        let byte = prefix.bytes[offset as usize];
        *total += 1;
        if byte == 0 {
            if offset == 0 {
                return Err(PeExportAddressError::EmptyForwarder { entry_index, start });
            }
            return Ok(std::str::from_utf8(&prefix.bytes[..offset as usize])
                .expect("each preceding byte was checked as ascii"));
        }
        if !byte.is_ascii() {
            return Err(PeExportAddressError::NonAsciiForwarder {
                entry_index,
                start,
                offset,
                byte,
            });
        }
        offset += 1;
    }
}

/// reads at most 4096 entries, retaining holes and unresolved direct rvas.
/// forwarders are classified by the directory range, without grammar checks.
/// string budgets include nul: 1024 bytes each and 65,536 across repeated scans.
///
/// # errors
/// validates the directory, count and whole table before ordered entries.
/// each ordinal precedes its target. string reads check local/global budgets,
/// the directory end, the whole physical prefix, then the byte; no partial table
/// is returned on failure.
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn parse_pe_export_addresses(
    bytes: &[u8],
) -> Result<Option<PeExportAddressTable<'_>>, PeExportAddressError> {
    let prepared = PreparedPe::new(bytes)
        .map_err(|cause| PeExportAddressError::Directory(PeExportDirectoryError::Base(cause)))?;
    let Some(directory) =
        parse_prepared_export_directory(&prepared).map_err(PeExportAddressError::Directory)?
    else {
        return Ok(None);
    };
    let count = directory.address_table_entries;
    if count > ENTRY_LIMIT {
        return Err(PeExportAddressError::EntryLimitExceeded {
            count,
            limit: ENTRY_LIMIT,
        });
    }
    let mut entries = Vec::new();
    if count == 0 {
        return Ok(Some(PeExportAddressTable { directory, entries }));
    }
    let start = directory.export_address_table_rva;
    if start.get() == 0 {
        return Err(PeExportAddressError::AddressTableUnavailable { count });
    }
    let length = count * 4;
    let table = prepared.resolve(start, length).map_err(|cause| {
        PeExportAddressError::AddressTableRange {
            start,
            length,
            cause,
        }
    })?;
    let directory_start = u64::from(directory.directory_rva.get());
    let directory_end = directory_start + u64::from(directory.directory_size);
    let mut total = 0;
    for table_index in 0..count {
        let ordinal = directory.ordinal_base.checked_add(table_index).ok_or(
            PeExportAddressError::OrdinalOverflow {
                ordinal_base: directory.ordinal_base,
                entry_index: table_index,
            },
        )?;
        let offset = table_index * 4;
        let raw = read_u32(table.bytes, offset as usize);
        let rva = RelativeVirtualAddress::new(raw);
        let target = if raw == 0 {
            PeExportTarget::Empty
        } else if (directory_start..directory_end).contains(&u64::from(raw)) {
            PeExportTarget::Forwarder {
                rva,
                text: read_forwarder(&prepared, table_index, rva, directory_end, &mut total)?,
            }
        } else {
            PeExportTarget::Rva(rva)
        };
        entries.push(PeExportAddressEntry {
            table_index,
            ordinal,
            entry_rva: RelativeVirtualAddress::new(start.get() + offset),
            entry_file_offset: FileOffset::new(table.file_offset.get() + u64::from(offset)),
            target,
        });
    }
    Ok(Some(PeExportAddressTable { directory, entries }))
}
