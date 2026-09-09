use super::optional::read_u16;
use super::resources::parse_prepared_resource_root;
use super::rva::PreparedPe;
use super::{PeResourceRoot, PeResourceRootError, PeRvaError};
use crate::{FileOffset, RelativeVirtualAddress};

const NAME_UNIT_LIMIT: u16 = 1024;
const TOTAL_UNIT_LIMIT: u32 = 32_768;

/// an exact unicode name record; bytes exclude the length field and stay undecoded.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeResourceRootName<'a> {
    pub entry_index: u16,
    pub name_offset: u32,
    pub name_rva: RelativeVirtualAddress,
    pub name_file_offset: FileOffset,
    pub code_unit_count: u16,
    pub utf16le: &'a [u8],
}

/// the raw root plus borrowed names for its declared named-entry prefix only.
///
/// ```compile_fail
/// use ring3_core::{PeResourceRootNameTable, parse_pe_resource_root_names};
///
/// fn escape() -> Option<PeResourceRootNameTable<'static>> {
///     let bytes = vec![0; 64];
///     parse_pe_resource_root_names(&bytes).unwrap()
/// }
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeResourceRootNameTable<'a> {
    pub root: PeResourceRoot,
    pub names: Vec<PeResourceRootName<'a>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeResourceRootNameError {
    Root(PeResourceRootError),
    UnsupportedNameEncoding {
        entry_index: u16,
        raw_name_or_id: u32,
    },
    NameOutsideDirectory {
        entry_index: u16,
        offset: u32,
        length: u32,
        directory_size: u32,
    },
    NameRange {
        entry_index: u16,
        start: RelativeVirtualAddress,
        length: u32,
        cause: PeRvaError,
    },
    NameLengthLimitExceeded {
        entry_index: u16,
        code_unit_count: u16,
        limit: u16,
    },
    NameCodeUnitBudgetExceeded {
        entry_index: u16,
        used: u32,
        code_unit_count: u16,
        limit: u32,
    },
}

fn read_name<'a>(
    prepared: &PreparedPe<'a>,
    root: &PeResourceRoot,
    entry_index: u16,
    used: u32,
) -> Result<PeResourceRootName<'a>, PeResourceRootNameError> {
    let raw_name_or_id = root.entries[usize::from(entry_index)].raw_name_or_id;
    if raw_name_or_id & 0x8000_0000 == 0 {
        return Err(PeResourceRootNameError::UnsupportedNameEncoding {
            entry_index,
            raw_name_or_id,
        });
    }
    let offset = raw_name_or_id & 0x7fff_ffff;
    let resolve = |length| {
        if u64::from(offset) + u64::from(length) > u64::from(root.directory_size) {
            return Err(PeResourceRootNameError::NameOutsideDirectory {
                entry_index,
                offset,
                length,
                directory_size: root.directory_size,
            });
        }
        let start = RelativeVirtualAddress::new(root.directory_rva.get() + offset);
        prepared
            .resolve(start, length)
            .map_err(|cause| PeResourceRootNameError::NameRange {
                entry_index,
                start,
                length,
                cause,
            })
    };
    let prefix = resolve(2)?;
    let code_unit_count = read_u16(prefix.bytes, 0);
    if code_unit_count > NAME_UNIT_LIMIT {
        return Err(PeResourceRootNameError::NameLengthLimitExceeded {
            entry_index,
            code_unit_count,
            limit: NAME_UNIT_LIMIT,
        });
    }
    if used + u32::from(code_unit_count) > TOTAL_UNIT_LIMIT {
        return Err(PeResourceRootNameError::NameCodeUnitBudgetExceeded {
            entry_index,
            used,
            code_unit_count,
            limit: TOTAL_UNIT_LIMIT,
        });
    }
    let record = resolve(2 + 2 * u32::from(code_unit_count))?;
    Ok(PeResourceRootName {
        entry_index,
        name_offset: offset,
        name_rva: RelativeVirtualAddress::new(root.directory_rva.get() + offset),
        name_file_offset: record.file_offset,
        code_unit_count,
        utf16le: &record.bytes[2..],
    })
}

/// reads root names with at most 1024 units each and 32768 units per call.
/// aliases consume the budget again; id rows and child/data targets stay raw.
/// no text decoding, normalization, null scan or alignment policy is applied.
///
/// # errors
/// validates the complete root first, then each name's encoding, relative prefix,
/// prefix backing, length limit, aggregate budget, full extent and full backing.
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn parse_pe_resource_root_names(
    bytes: &[u8],
) -> Result<Option<PeResourceRootNameTable<'_>>, PeResourceRootNameError> {
    let prepared = PreparedPe::new(bytes)
        .map_err(|cause| PeResourceRootNameError::Root(PeResourceRootError::Base(cause)))?;
    let Some(root) =
        parse_prepared_resource_root(&prepared).map_err(PeResourceRootNameError::Root)?
    else {
        return Ok(None);
    };
    let mut names = Vec::with_capacity(usize::from(root.number_of_named_entries));
    let mut used = 0;
    for entry_index in 0..root.number_of_named_entries {
        let name = read_name(&prepared, &root, entry_index, used)?;
        used += u32::from(name.code_unit_count);
        names.push(name);
    }
    Ok(Some(PeResourceRootNameTable { root, names }))
}
