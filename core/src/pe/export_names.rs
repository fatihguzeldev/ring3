use super::export_addresses::parse_prepared_export_addresses;
use super::optional::{read_u16, read_u32};
use super::rva::PreparedPe;
use super::{PeExportAddressError, PeExportAddressTable, PeExportDirectoryError, PeRvaError};
use crate::{FileOffset, RelativeVirtualAddress};

const NAME_LIMIT: u32 = 4096;
const NAME_LENGTH_LIMIT: u32 = 1024;
const NAME_SCAN_BUDGET: u32 = 65_536;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeExportName<'a> {
    pub table_index: u32,
    pub name_pointer_rva: RelativeVirtualAddress,
    pub name_pointer_file_offset: FileOffset,
    pub ordinal_entry_rva: RelativeVirtualAddress,
    pub ordinal_entry_file_offset: FileOffset,
    pub address_index: u16,
    pub name_rva: RelativeVirtualAddress,
    pub name_file_offset: FileOffset,
    pub name: &'a str,
}

/// raw names and unbiased indexes into the retained address table.
///
/// ```compile_fail
/// use ring3_core::{PeExportNameTable, parse_pe_export_names};
///
/// fn escape() -> Option<PeExportNameTable<'static>> {
///     let bytes = vec![0; 64];
///     parse_pe_export_names(&bytes).unwrap()
/// }
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeExportNameTable<'a> {
    pub addresses: PeExportAddressTable<'a>,
    pub entries: Vec<PeExportName<'a>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeExportNameError {
    Addresses(PeExportAddressError),
    NameLimitExceeded {
        count: u32,
        limit: u32,
    },
    NamePointerTableUnavailable {
        count: u32,
    },
    OrdinalTableUnavailable {
        count: u32,
    },
    NamePointerTableRange {
        start: RelativeVirtualAddress,
        length: u32,
        cause: PeRvaError,
    },
    OrdinalTableRange {
        start: RelativeVirtualAddress,
        length: u32,
        cause: PeRvaError,
    },
    AddressIndexOutOfRange {
        entry_index: u32,
        address_index: u16,
        address_count: u32,
    },
    NameUnavailable {
        entry_index: u32,
    },
    NameRange {
        entry_index: u32,
        start: RelativeVirtualAddress,
        offset: u32,
        cause: PeRvaError,
    },
    EmptyName {
        entry_index: u32,
        start: RelativeVirtualAddress,
    },
    NonAsciiName {
        entry_index: u32,
        start: RelativeVirtualAddress,
        offset: u32,
        byte: u8,
    },
    NameLengthLimitExceeded {
        entry_index: u32,
        start: RelativeVirtualAddress,
        limit: u32,
    },
    NameScanBudgetExceeded {
        entry_index: u32,
        start: RelativeVirtualAddress,
        offset: u32,
        limit: u32,
    },
}

fn read_name<'a>(
    prepared: &PreparedPe<'a>,
    entry_index: u32,
    start: RelativeVirtualAddress,
    total: &mut u32,
) -> Result<(FileOffset, &'a str), PeExportNameError> {
    let mut offset = 0;
    loop {
        if offset == NAME_LENGTH_LIMIT {
            return Err(PeExportNameError::NameLengthLimitExceeded {
                entry_index,
                start,
                limit: NAME_LENGTH_LIMIT,
            });
        }
        if *total == NAME_SCAN_BUDGET {
            return Err(PeExportNameError::NameScanBudgetExceeded {
                entry_index,
                start,
                offset,
                limit: NAME_SCAN_BUDGET,
            });
        }
        let prefix =
            prepared
                .resolve(start, offset + 1)
                .map_err(|cause| PeExportNameError::NameRange {
                    entry_index,
                    start,
                    offset,
                    cause,
                })?;
        let byte = prefix.bytes[offset as usize];
        *total += 1;
        if byte == 0 {
            if offset == 0 {
                return Err(PeExportNameError::EmptyName { entry_index, start });
            }
            let name = std::str::from_utf8(&prefix.bytes[..offset as usize])
                .expect("each preceding byte was checked as ascii");
            return Ok((prefix.file_offset, name));
        }
        if !byte.is_ascii() {
            return Err(PeExportNameError::NonAsciiName {
                entry_index,
                start,
                offset,
                byte,
            });
        }
        offset += 1;
    }
}

/// reads at most 4096 ordered names without sorting, deduplication or lookup.
/// names may be outside the directory and may refer to empty address entries.
/// name budgets include nul and duplicate scans: 1024 bytes each and 65,536
/// total, separate from the inherited forwarder budget.
///
/// # errors
/// validates all addresses, name count, both sources, and both complete tables
/// before ordered rows. each index precedes its name pointer and string. string
/// reads check local/global budgets, whole physical prefix, then byte; failures
/// return no partial table.
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn parse_pe_export_names(
    bytes: &[u8],
) -> Result<Option<PeExportNameTable<'_>>, PeExportNameError> {
    let prepared = PreparedPe::new(bytes).map_err(|cause| {
        PeExportNameError::Addresses(PeExportAddressError::Directory(
            PeExportDirectoryError::Base(cause),
        ))
    })?;
    let Some(addresses) =
        parse_prepared_export_addresses(&prepared).map_err(PeExportNameError::Addresses)?
    else {
        return Ok(None);
    };
    let count = addresses.directory.number_of_name_pointers;
    if count > NAME_LIMIT {
        return Err(PeExportNameError::NameLimitExceeded {
            count,
            limit: NAME_LIMIT,
        });
    }
    let mut entries = Vec::new();
    if count == 0 {
        return Ok(Some(PeExportNameTable { addresses, entries }));
    }
    let name_start = addresses.directory.name_pointer_rva;
    let ordinal_start = addresses.directory.ordinal_table_rva;
    if name_start.get() == 0 {
        return Err(PeExportNameError::NamePointerTableUnavailable { count });
    }
    if ordinal_start.get() == 0 {
        return Err(PeExportNameError::OrdinalTableUnavailable { count });
    }
    let name_length = count * 4;
    let names = prepared.resolve(name_start, name_length).map_err(|cause| {
        PeExportNameError::NamePointerTableRange {
            start: name_start,
            length: name_length,
            cause,
        }
    })?;
    let ordinal_length = count * 2;
    let ordinals = prepared
        .resolve(ordinal_start, ordinal_length)
        .map_err(|cause| PeExportNameError::OrdinalTableRange {
            start: ordinal_start,
            length: ordinal_length,
            cause,
        })?;
    let mut total = 0;
    for table_index in 0..count {
        let name_offset = table_index * 4;
        let ordinal_offset = table_index * 2;
        let address_index = read_u16(ordinals.bytes, ordinal_offset as usize);
        if usize::from(address_index) >= addresses.entries.len() {
            return Err(PeExportNameError::AddressIndexOutOfRange {
                entry_index: table_index,
                address_index,
                address_count: addresses.directory.address_table_entries,
            });
        }
        let name_rva = RelativeVirtualAddress::new(read_u32(names.bytes, name_offset as usize));
        if name_rva.get() == 0 {
            return Err(PeExportNameError::NameUnavailable {
                entry_index: table_index,
            });
        }
        let (name_file_offset, name) = read_name(&prepared, table_index, name_rva, &mut total)?;
        entries.push(PeExportName {
            table_index,
            name_pointer_rva: RelativeVirtualAddress::new(name_start.get() + name_offset),
            name_pointer_file_offset: FileOffset::new(
                names.file_offset.get() + u64::from(name_offset),
            ),
            ordinal_entry_rva: RelativeVirtualAddress::new(ordinal_start.get() + ordinal_offset),
            ordinal_entry_file_offset: FileOffset::new(
                ordinals.file_offset.get() + u64::from(ordinal_offset),
            ),
            address_index,
            name_rva,
            name_file_offset,
            name,
        });
    }
    Ok(Some(PeExportNameTable { addresses, entries }))
}
