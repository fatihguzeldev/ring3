use super::resource_directories::parse_prepared_resource_directories;
use super::resource_names::{PeResourceRootNameError, read_resource_name};
use super::rva::PreparedPe;
use super::{PeResourceDirectoryError, PeResourceDirectoryGraph, PeResourceRootError, PeRvaError};
use crate::{FileOffset, RelativeVirtualAddress};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeResourceDirectoryName<'a> {
    pub directory_index: u16,
    pub entry_index: u16,
    pub name_offset: u32,
    pub name_rva: RelativeVirtualAddress,
    pub name_file_offset: FileOffset,
    pub code_unit_count: u16,
    pub utf16le: &'a [u8],
}

/// borrowed raw names for each unique directory's declared named-entry prefix.
///
/// ```compile_fail
/// use ring3_core::{PeResourceDirectoryNameTable, parse_pe_resource_directory_names};
///
/// fn escape() -> Option<PeResourceDirectoryNameTable<'static>> {
///     let bytes = vec![0; 64];
///     parse_pe_resource_directory_names(&bytes).unwrap()
/// }
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeResourceDirectoryNameTable<'a> {
    pub directory_graph: PeResourceDirectoryGraph,
    pub names: Vec<PeResourceDirectoryName<'a>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeResourceDirectoryNameError {
    Graph(PeResourceDirectoryError),
    UnsupportedNameEncoding {
        directory_index: u16,
        entry_index: u16,
        raw_name_or_id: u32,
    },
    NameOutsideDirectory {
        directory_index: u16,
        entry_index: u16,
        offset: u32,
        length: u32,
        directory_size: u32,
    },
    NameRange {
        directory_index: u16,
        entry_index: u16,
        start: RelativeVirtualAddress,
        length: u32,
        cause: PeRvaError,
    },
    NameLengthLimitExceeded {
        directory_index: u16,
        entry_index: u16,
        code_unit_count: u16,
        limit: u16,
    },
    NameCodeUnitBudgetExceeded {
        directory_index: u16,
        entry_index: u16,
        used: u32,
        code_unit_count: u16,
        limit: u32,
    },
}

fn directory_error(
    directory_index: u16,
    error: PeResourceRootNameError,
) -> PeResourceDirectoryNameError {
    match error {
        PeResourceRootNameError::Root(cause) => {
            PeResourceDirectoryNameError::Graph(PeResourceDirectoryError::Root(cause))
        }
        PeResourceRootNameError::UnsupportedNameEncoding {
            entry_index,
            raw_name_or_id,
        } => PeResourceDirectoryNameError::UnsupportedNameEncoding {
            directory_index,
            entry_index,
            raw_name_or_id,
        },
        PeResourceRootNameError::NameOutsideDirectory {
            entry_index,
            offset,
            length,
            directory_size,
        } => PeResourceDirectoryNameError::NameOutsideDirectory {
            directory_index,
            entry_index,
            offset,
            length,
            directory_size,
        },
        PeResourceRootNameError::NameRange {
            entry_index,
            start,
            length,
            cause,
        } => PeResourceDirectoryNameError::NameRange {
            directory_index,
            entry_index,
            start,
            length,
            cause,
        },
        PeResourceRootNameError::NameLengthLimitExceeded {
            entry_index,
            code_unit_count,
            limit,
        } => PeResourceDirectoryNameError::NameLengthLimitExceeded {
            directory_index,
            entry_index,
            code_unit_count,
            limit,
        },
        PeResourceRootNameError::NameCodeUnitBudgetExceeded {
            entry_index,
            used,
            code_unit_count,
            limit,
        } => PeResourceDirectoryNameError::NameCodeUnitBudgetExceeded {
            directory_index,
            entry_index,
            used,
            code_unit_count,
            limit,
        },
    }
}

/// reads undecoded name bytes throughout one supported resource directory graph.
/// names have at most 1024 units each and 32768 units per call, counting repeated
/// name offsets again. shared directory paths are not expanded. id rows and all
/// payload targets stay unread; no decoding, normalization or alignment rule is
/// applied. graph entry limits bound the number of returned name records.
///
/// # errors
/// validates the entire graph first. for each declared named row, checks encoding,
/// relative prefix and backing, unit limit and aggregate budget, then complete
/// relative extent and backing. errors retain directory and entry attribution.
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn parse_pe_resource_directory_names(
    bytes: &[u8],
) -> Result<Option<PeResourceDirectoryNameTable<'_>>, PeResourceDirectoryNameError> {
    let prepared = PreparedPe::new(bytes).map_err(|cause| {
        PeResourceDirectoryNameError::Graph(PeResourceDirectoryError::Root(
            PeResourceRootError::Base(cause),
        ))
    })?;
    let Some(directory_graph) = parse_prepared_resource_directories(&prepared)
        .map_err(PeResourceDirectoryNameError::Graph)?
    else {
        return Ok(None);
    };
    let mut names = Vec::new();
    let mut used = 0;
    for (directory_index, directory) in (0_u16..).zip(&directory_graph.directories) {
        for entry_index in 0..directory.number_of_named_entries {
            let raw_name_or_id = directory.entries[usize::from(entry_index)].raw_name_or_id;
            let name = read_resource_name(
                &prepared,
                directory_graph.directory_rva,
                directory_graph.directory_size,
                entry_index,
                raw_name_or_id,
                used,
            )
            .map_err(|cause| directory_error(directory_index, cause))?;
            used += u32::from(name.code_unit_count);
            names.push(PeResourceDirectoryName {
                directory_index,
                entry_index,
                name_offset: name.name_offset,
                name_rva: name.name_rva,
                name_file_offset: name.name_file_offset,
                code_unit_count: name.code_unit_count,
                utf16le: name.utf16le,
            });
        }
    }
    Ok(Some(PeResourceDirectoryNameTable {
        directory_graph,
        names,
    }))
}
