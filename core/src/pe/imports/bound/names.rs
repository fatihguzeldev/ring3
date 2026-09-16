use super::{PeBoundImportError, PeBoundImportTable, parse_prepared_table};
use crate::pe::rva::PreparedPe;
use crate::{FileOffset, PeRvaError, RelativeVirtualAddress};

/// an index into the validated raw table, in descriptor-then-reference order.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeBoundImportNameLocation {
    Descriptor {
        descriptor_index: u16,
    },
    Forwarder {
        descriptor_index: u16,
        forwarder_index: u16,
    },
}

/// exact nonempty ascii bytes borrowed from the input, excluding the nul.
/// spelling and control bytes remain unchanged; this is not path validation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeBoundImportName<'a> {
    pub location: PeBoundImportNameLocation,
    pub name_rva: RelativeVirtualAddress,
    pub name_file_offset: FileOffset,
    pub dll_name: &'a str,
}

/// the owned raw table and ordered borrowed names, including duplicate offsets.
/// mutating public fields may invalidate the original location-to-record pairing.
///
/// ```compile_fail
/// use ring3_core::{PeBoundImportNameTable, parse_pe_bound_import_names};
/// fn escape() -> Option<PeBoundImportNameTable<'static>> {
///     let bytes = vec![0; 64];
///     parse_pe_bound_import_names(&bytes).unwrap()
/// }
/// ```
///
/// ```compile_fail
/// use ring3_core::parse_pe_bound_import_names;
/// fn mutate(bytes: &mut [u8]) -> usize {
///     let table = parse_pe_bound_import_names(bytes).unwrap().unwrap();
///     bytes.fill(0);
///     table.names.len()
/// }
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeBoundImportNameTable<'a> {
    pub table: PeBoundImportTable,
    pub names: Vec<PeBoundImportName<'a>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeBoundImportNameError {
    Table(PeBoundImportError),
    NameRvaOverflow {
        location: PeBoundImportNameLocation,
        directory_rva: RelativeVirtualAddress,
        module_name_offset: u16,
    },
    NameRange {
        location: PeBoundImportNameLocation,
        name_rva: RelativeVirtualAddress,
        offset: u32,
        cause: PeRvaError,
    },
    EmptyDllName {
        location: PeBoundImportNameLocation,
        name_rva: RelativeVirtualAddress,
    },
    NonAsciiDllName {
        location: PeBoundImportNameLocation,
        name_rva: RelativeVirtualAddress,
        offset: u32,
        byte: u8,
    },
    NameLengthLimitExceeded {
        location: PeBoundImportNameLocation,
        name_rva: RelativeVirtualAddress,
        limit: u32,
    },
    NameScanBudgetExceeded {
        location: PeBoundImportNameLocation,
        name_rva: RelativeVirtualAddress,
        offset: u32,
        limit: u32,
    },
}

const NAME_SCAN_LIMIT: u32 = 1024;
const TOTAL_SCAN_LIMIT: u32 = 65_536;

fn read_name<'a>(
    prepared: &PreparedPe<'a>,
    location: PeBoundImportNameLocation,
    directory_rva: RelativeVirtualAddress,
    module_name_offset: u16,
    total: &mut u32,
) -> Result<PeBoundImportName<'a>, PeBoundImportNameError> {
    let name_rva = directory_rva
        .get()
        .checked_add(u32::from(module_name_offset))
        .map(RelativeVirtualAddress::new)
        .ok_or(PeBoundImportNameError::NameRvaOverflow {
            location,
            directory_rva,
            module_name_offset,
        })?;
    for offset in 0..=NAME_SCAN_LIMIT {
        if offset == NAME_SCAN_LIMIT {
            return Err(PeBoundImportNameError::NameLengthLimitExceeded {
                location,
                name_rva,
                limit: NAME_SCAN_LIMIT,
            });
        }
        if *total == TOTAL_SCAN_LIMIT {
            return Err(PeBoundImportNameError::NameScanBudgetExceeded {
                location,
                name_rva,
                offset,
                limit: TOTAL_SCAN_LIMIT,
            });
        }
        let prefix = prepared.resolve(name_rva, offset + 1).map_err(|cause| {
            PeBoundImportNameError::NameRange {
                location,
                name_rva,
                offset,
                cause,
            }
        })?;
        let byte = prefix.bytes[offset as usize];
        *total += 1;
        if byte == 0 {
            if offset == 0 {
                return Err(PeBoundImportNameError::EmptyDllName { location, name_rva });
            }
            return Ok(PeBoundImportName {
                location,
                name_rva,
                name_file_offset: prefix.file_offset,
                dll_name: std::str::from_utf8(&prefix.bytes[..offset as usize])
                    .expect("all preceding bytes passed the ascii check"),
            });
        }
        if !byte.is_ascii() {
            return Err(PeBoundImportNameError::NonAsciiDllName {
                location,
                name_rva,
                offset,
                byte,
            });
        }
    }
    unreachable!("the final name offset refuses before reading");
}

/// reads names only after the complete bounded raw bound-import table validates.
/// the raw table is owned once; each descriptor name precedes its reference names.
/// names borrow the original input, with at most 1152 occurrences and no deduplication.
/// each u16 name offset is relative to the directory rva. starts at offset zero,
/// inside records, outside the declared directory or in another region are allowed
/// when the whole name prefix has one conservative file-backed range.
///
/// each name may consume 1024 bytes and all occurrences together 65536 bytes,
/// counting nul terminators and duplicate reads. accepted text is 1..=1023 ascii
/// bytes; controls, slashes and case are preserved. these limits exclude input
/// size, base parsing, repeated resolver work and allocator overhead. they are
/// not whole-call memory or time bounds; this synchronous reader has no cancellation.
/// names and timestamps do not establish provider identity or binding validity.
///
/// ```
/// use ring3_core::{PeBoundImportNameError, parse_pe_bound_import_names};
/// assert!(matches!(
///     parse_pe_bound_import_names(&[]),
///     Err(PeBoundImportNameError::Table(_))
/// ));
/// ```
///
/// # errors
/// base and complete raw-table errors precede all name allocation and reads.
/// per occurrence: checked start, local cap, aggregate cap, whole-prefix backing,
/// then charged byte, nul and ascii checks. no error returns partial observations.
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn parse_pe_bound_import_names(
    bytes: &[u8],
) -> Result<Option<PeBoundImportNameTable<'_>>, PeBoundImportNameError> {
    let prepared = PreparedPe::new(bytes)
        .map_err(|cause| PeBoundImportNameError::Table(PeBoundImportError::Base(cause)))?;
    let Some(table) = parse_prepared_table(&prepared).map_err(PeBoundImportNameError::Table)?
    else {
        return Ok(None);
    };
    let mut names = Vec::new();
    let mut total = 0;
    for (descriptor_index, descriptor) in (0_u16..).zip(&table.descriptors) {
        names.push(read_name(
            &prepared,
            PeBoundImportNameLocation::Descriptor { descriptor_index },
            table.directory_rva,
            descriptor.module_name_offset,
            &mut total,
        )?);
        for (forwarder_index, reference) in (0_u16..).zip(&descriptor.forwarder_refs) {
            names.push(read_name(
                &prepared,
                PeBoundImportNameLocation::Forwarder {
                    descriptor_index,
                    forwarder_index,
                },
                table.directory_rva,
                reference.module_name_offset,
                &mut total,
            )?);
        }
    }
    Ok(Some(PeBoundImportNameTable { table, names }))
}
