use super::optional::read_u32;
use super::rva::PreparedPe;
use super::{PeDirectoryAddress, PeRvaError};
use crate::{FileOffset, RelativeVirtualAddress};

const DESCRIPTOR_LIMIT: u16 = 128;
const NAME_LENGTH_LIMIT: u32 = 1024;
const NAME_SCAN_BUDGET: u32 = 65_536;

/// raw static import metadata and an exact same-input name; no thunk traversal.
///
/// ```compile_fail
/// use ring3_core::{PeImportDescriptor, parse_pe_import_descriptors};
///
/// fn escape() -> Vec<PeImportDescriptor<'static>> {
///     let bytes = vec![0; 64];
///     parse_pe_import_descriptors(&bytes).unwrap()
/// }
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeImportDescriptor<'a> {
    pub descriptor_rva: RelativeVirtualAddress,
    pub descriptor_file_offset: FileOffset,
    pub import_lookup_table_rva: RelativeVirtualAddress,
    pub time_date_stamp: u32,
    pub forwarder_chain: u32,
    pub name_rva: RelativeVirtualAddress,
    pub import_address_table_rva: RelativeVirtualAddress,
    pub dll_name: &'a str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeImportError {
    Base(PeRvaError),
    InconsistentImportDirectory {
        rva: RelativeVirtualAddress,
        size: u32,
    },
    ImportDirectoryRangeOverflow {
        rva: RelativeVirtualAddress,
        size: u32,
    },
    TruncatedImportDescriptor {
        descriptor_index: u16,
        remaining: u32,
    },
    MissingImportTerminator {
        descriptor_index: u16,
    },
    ImportDescriptorLimitExceeded {
        descriptor_index: u16,
        limit: u16,
    },
    DescriptorRange {
        descriptor_index: u16,
        start: RelativeVirtualAddress,
        length: u32,
        cause: PeRvaError,
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
) -> Result<&'a str, PeImportError> {
    let mut offset = 0;
    loop {
        if offset == NAME_LENGTH_LIMIT {
            return Err(PeImportError::NameLengthLimitExceeded {
                descriptor_index,
                name_rva,
                limit: NAME_LENGTH_LIMIT,
            });
        }
        if *total == NAME_SCAN_BUDGET {
            return Err(PeImportError::NameScanBudgetExceeded {
                descriptor_index,
                name_rva,
                offset,
                limit: NAME_SCAN_BUDGET,
            });
        }
        let prefix =
            prepared
                .resolve(name_rva, offset + 1)
                .map_err(|cause| PeImportError::NameRange {
                    descriptor_index,
                    name_rva,
                    offset,
                    cause,
                })?;
        let byte = prefix.bytes[offset as usize];
        *total += 1;
        if byte == 0 {
            if offset == 0 {
                return Err(PeImportError::EmptyDllName {
                    descriptor_index,
                    name_rva,
                });
            }
            return Ok(std::str::from_utf8(&prefix.bytes[..offset as usize])
                .expect("each preceding byte passed the ASCII check"));
        }
        if !byte.is_ascii() {
            return Err(PeImportError::NonAsciiDllName {
                descriptor_index,
                name_rva,
                offset,
                byte,
            });
        }
        offset += 1;
    }
}

/// reads at most 128 static descriptors and borrowed, nonempty ascii dll names.
/// names may be outside the directory, but each consumed table/name prefix must
/// resolve as one conservative file-backed range. budgets count nul: 1024 bytes
/// per name and 65,536 total, including repeated scans of duplicate names.
///
/// # errors
/// validates the base file first, then directory arithmetic, each descriptor,
/// and its name in order. per-name budget precedes total budget; no partial
/// descriptor list is returned on failure.
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn parse_pe_import_descriptors(
    bytes: &[u8],
) -> Result<Vec<PeImportDescriptor<'_>>, PeImportError> {
    let prepared = PreparedPe::new(bytes).map_err(PeImportError::Base)?;
    parse_prepared_descriptors(&prepared)
}

pub(super) fn parse_prepared_descriptors<'a>(
    prepared: &PreparedPe<'a>,
) -> Result<Vec<PeImportDescriptor<'a>>, PeImportError> {
    let Some(directory) = prepared.headers().directories[1] else {
        return Ok(Vec::new());
    };
    let PeDirectoryAddress::Rva(rva) = directory.address else {
        unreachable!("the same-input parser uses RVA coordinates for import slot 1");
    };
    let size = directory.size;
    if rva.get() == 0 && size == 0 {
        return Ok(Vec::new());
    }
    if rva.get() == 0 || size == 0 {
        return Err(PeImportError::InconsistentImportDirectory { rva, size });
    }
    if u64::from(rva.get()) + u64::from(size) > 1_u64 << 32 {
        return Err(PeImportError::ImportDirectoryRangeOverflow { rva, size });
    }
    let mut descriptors = Vec::new();
    let mut total_name_bytes = 0;
    for descriptor_index in 0..=DESCRIPTOR_LIMIT {
        let offset = u32::from(descriptor_index) * 20;
        let remaining = size - offset;
        if remaining == 0 {
            return Err(PeImportError::MissingImportTerminator { descriptor_index });
        }
        if remaining < 20 {
            return Err(PeImportError::TruncatedImportDescriptor {
                descriptor_index,
                remaining,
            });
        }
        let length = offset + 20;
        let prefix =
            prepared
                .resolve(rva, length)
                .map_err(|cause| PeImportError::DescriptorRange {
                    descriptor_index,
                    start: rva,
                    length,
                    cause,
                })?;
        let record = &prefix.bytes[offset as usize..];
        let fields = [0, 4, 8, 12, 16].map(|index| read_u32(record, index));
        if fields == [0; 5] {
            return Ok(descriptors);
        }
        if descriptor_index == DESCRIPTOR_LIMIT {
            return Err(PeImportError::ImportDescriptorLimitExceeded {
                descriptor_index,
                limit: DESCRIPTOR_LIMIT,
            });
        }
        let name_rva = RelativeVirtualAddress::new(fields[3]);
        let dll_name = read_name(prepared, descriptor_index, name_rva, &mut total_name_bytes)?;
        descriptors.push(PeImportDescriptor {
            descriptor_rva: RelativeVirtualAddress::new(rva.get() + offset),
            descriptor_file_offset: FileOffset::new(prefix.file_offset.get() + u64::from(offset)),
            import_lookup_table_rva: RelativeVirtualAddress::new(fields[0]),
            time_date_stamp: fields[1],
            forwarder_chain: fields[2],
            name_rva,
            import_address_table_rva: RelativeVirtualAddress::new(fields[4]),
            dll_name,
        });
    }
    unreachable!("the last allowed record returns a terminator or a limit error")
}
