use super::delay_imports::parse_prepared_descriptors;
use super::rva::PreparedPe;
use super::{PeDelayImportDescriptor, PeDelayImportError, PeKind, PeRvaError};
use crate::{FileOffset, RelativeVirtualAddress};

const NAME_LENGTH_LIMIT: u32 = 1024;
const NAME_SCAN_BUDGET: u32 = 65_536;

/// a raw descriptor paired with an exact same-input dll name; no symbol lookup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeDelayImportName<'a> {
    pub descriptor: PeDelayImportDescriptor,
    pub dll_name: &'a str,
}

/// names borrow the original file and retain raw table order and coordinates.
///
/// ```compile_fail
/// use ring3_core::{PeDelayImportNameTable, parse_pe_delay_import_names};
///
/// fn escape() -> PeDelayImportNameTable<'static> {
///     let bytes = vec![0; 64];
///     parse_pe_delay_import_names(&bytes).unwrap().unwrap()
/// }
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeDelayImportNameTable<'a> {
    pub kind: PeKind,
    pub directory_rva: RelativeVirtualAddress,
    pub directory_file_offset: FileOffset,
    pub directory_size: u32,
    pub imports: Vec<PeDelayImportName<'a>>,
    pub terminator_rva: RelativeVirtualAddress,
    pub terminator_file_offset: FileOffset,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeDelayImportNameError {
    Table(PeDelayImportError),
    UnsupportedAttributes {
        descriptor_index: u16,
        attributes: u32,
    },
    NameRange {
        descriptor_index: u16,
        name_rva: RelativeVirtualAddress,
        offset: u32,
        cause: PeRvaError,
    },
    EmptyDllName {
        descriptor_index: u16,
        name_rva: RelativeVirtualAddress,
    },
    NonAsciiDllName {
        descriptor_index: u16,
        name_rva: RelativeVirtualAddress,
        offset: u32,
        byte: u8,
    },
    NameLengthLimitExceeded {
        descriptor_index: u16,
        name_rva: RelativeVirtualAddress,
        limit: u32,
    },
    NameScanBudgetExceeded {
        descriptor_index: u16,
        name_rva: RelativeVirtualAddress,
        offset: u32,
        limit: u32,
    },
}

fn read_name<'a>(
    prepared: &PreparedPe<'a>,
    descriptor_index: u16,
    name_rva: RelativeVirtualAddress,
    total: &mut u32,
) -> Result<&'a str, PeDelayImportNameError> {
    for offset in 0..=NAME_LENGTH_LIMIT {
        if offset == NAME_LENGTH_LIMIT {
            return Err(PeDelayImportNameError::NameLengthLimitExceeded {
                descriptor_index,
                name_rva,
                limit: NAME_LENGTH_LIMIT,
            });
        }
        if *total == NAME_SCAN_BUDGET {
            return Err(PeDelayImportNameError::NameScanBudgetExceeded {
                descriptor_index,
                name_rva,
                offset,
                limit: NAME_SCAN_BUDGET,
            });
        }
        let prefix = prepared.resolve(name_rva, offset + 1).map_err(|cause| {
            PeDelayImportNameError::NameRange {
                descriptor_index,
                name_rva,
                offset,
                cause,
            }
        })?;
        let byte = prefix.bytes[offset as usize];
        *total += 1;
        if byte == 0 {
            if offset == 0 {
                return Err(PeDelayImportNameError::EmptyDllName {
                    descriptor_index,
                    name_rva,
                });
            }
            return Ok(std::str::from_utf8(&prefix.bytes[..offset as usize])
                .expect("all preceding bytes passed the ascii check"));
        }
        if !byte.is_ascii() {
            return Err(PeDelayImportNameError::NonAsciiDllName {
                descriptor_index,
                name_rva,
                offset,
                byte,
            });
        }
    }
    unreachable!("the final name offset refuses before reading");
}

/// reads nonempty ascii dll names only for descriptors with attributes exactly 1.
/// name scans count nul: at most 1024 bytes each and 65,536 total, including
/// duplicate scans. every consumed name prefix must be one conservative range.
/// names may be outside the directory; no other address targets are followed.
/// zero name rvas use header backing, and names are neither normalized nor resolved.
///
/// # errors
/// completes the raw table first, then checks each descriptor's attributes and name.
/// per-name budget precedes total budget and backing; failure returns no partial table.
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn parse_pe_delay_import_names(
    bytes: &[u8],
) -> Result<Option<PeDelayImportNameTable<'_>>, PeDelayImportNameError> {
    let prepared = PreparedPe::new(bytes)
        .map_err(|cause| PeDelayImportNameError::Table(PeDelayImportError::Base(cause)))?;
    let Some(table) =
        parse_prepared_descriptors(&prepared).map_err(PeDelayImportNameError::Table)?
    else {
        return Ok(None);
    };
    let mut imports = Vec::new();
    let mut total = 0;
    for (descriptor_index, descriptor) in (0_u16..).zip(table.descriptors) {
        if descriptor.attributes != 1 {
            return Err(PeDelayImportNameError::UnsupportedAttributes {
                descriptor_index,
                attributes: descriptor.attributes,
            });
        }
        let name_rva = RelativeVirtualAddress::new(descriptor.dll_name_address);
        let dll_name = read_name(&prepared, descriptor_index, name_rva, &mut total)?;
        imports.push(PeDelayImportName {
            descriptor,
            dll_name,
        });
    }
    Ok(Some(PeDelayImportNameTable {
        kind: table.kind,
        directory_rva: table.directory_rva,
        directory_file_offset: table.directory_file_offset,
        directory_size: table.directory_size,
        imports,
        terminator_rva: table.terminator_rva,
        terminator_file_offset: table.terminator_file_offset,
    }))
}
