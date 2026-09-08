use super::delay_names::parse_prepared_names;
use super::lookups::lookup_entries;
use super::rva::PreparedPe;
use super::{
    PeDelayImportError, PeDelayImportName, PeDelayImportNameError, PeImportLookupEntry,
    PeImportLookupError, PeKind,
};
use crate::{FileOffset, RelativeVirtualAddress};

/// raw delay metadata and same-input names, in descriptor and lookup order.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeDelayImportLookup<'a> {
    pub import: PeDelayImportName<'a>,
    pub entries: Vec<PeImportLookupEntry<'a>>,
}

/// borrowed symbol identities; no module resolution or delayed execution.
///
/// ```compile_fail
/// use ring3_core::{PeDelayImportLookupTable, parse_pe_delay_import_lookups};
///
/// fn escape() -> PeDelayImportLookupTable<'static> {
///     let bytes = vec![0; 64];
///     parse_pe_delay_import_lookups(&bytes).unwrap().unwrap()
/// }
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeDelayImportLookupTable<'a> {
    pub kind: PeKind,
    pub directory_rva: RelativeVirtualAddress,
    pub directory_file_offset: FileOffset,
    pub directory_size: u32,
    pub imports: Vec<PeDelayImportLookup<'a>>,
    pub terminator_rva: RelativeVirtualAddress,
    pub terminator_file_offset: FileOffset,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeDelayImportLookupError {
    Names(PeDelayImportNameError),
    /// shared entry failures; this reader never emits `PeImportLookupError::Descriptors`.
    Lookup(PeImportLookupError),
}

/// reads only explicit delay lookup tables after validating all raw descriptors
/// and supported dll names. zero int coordinates never fall back to the iat.
/// entries share the static reader's width, encoding and conservative backing rules.
/// limits are 1024 entries per dll, 4096 total, and 1024/65,536 symbol-name bytes
/// including nul; hints and the independent dll-name scans do not consume that budget.
///
/// # errors
/// raw table and all dll-name failures precede lookup work. fetched zero entries
/// precede entry budgets; local budgets precede global budgets, and name budgets
/// precede hint/name reads. errors retain original indices and return no partial table.
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn parse_pe_delay_import_lookups(
    bytes: &[u8],
) -> Result<Option<PeDelayImportLookupTable<'_>>, PeDelayImportLookupError> {
    let prepared = PreparedPe::new(bytes).map_err(|cause| {
        PeDelayImportLookupError::Names(PeDelayImportNameError::Table(PeDelayImportError::Base(
            cause,
        )))
    })?;
    let Some(table) = parse_prepared_names(&prepared).map_err(PeDelayImportLookupError::Names)?
    else {
        return Ok(None);
    };
    let mut imports = Vec::new();
    let mut total_entries = 0;
    let mut total_name_bytes = 0;
    for (descriptor_index, import) in (0_u16..).zip(table.imports) {
        let start = RelativeVirtualAddress::new(import.descriptor.import_name_table_address);
        if start.get() == 0 {
            return Err(PeDelayImportLookupError::Lookup(
                PeImportLookupError::LookupTableUnavailable { descriptor_index },
            ));
        }
        let entries = lookup_entries(
            &prepared,
            descriptor_index,
            start,
            &mut total_entries,
            &mut total_name_bytes,
        )
        .map_err(PeDelayImportLookupError::Lookup)?;
        imports.push(PeDelayImportLookup { import, entries });
    }
    Ok(Some(PeDelayImportLookupTable {
        kind: table.kind,
        directory_rva: table.directory_rva,
        directory_file_offset: table.directory_file_offset,
        directory_size: table.directory_size,
        imports,
        terminator_rva: table.terminator_rva,
        terminator_file_offset: table.terminator_file_offset,
    }))
}
