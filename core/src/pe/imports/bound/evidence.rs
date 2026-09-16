use super::{
    PeBoundImportError, PeBoundImportNameError, PeBoundImportNameLocation, PeBoundImportNameTable,
    PeBoundImportTable, parse_pe_bound_import_descriptors, parse_pe_bound_import_names,
};
use crate::{FileOffset, RelativeVirtualAddress};

/// per-call input and logical output caps; not allocation or memory limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeBoundImportEvidenceLimits {
    pub max_input_bytes: u64,
    pub max_output_rows: u64,
    pub max_output_text_bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeBoundImportEvidenceError {
    InputTooLarge { length: u64, limit: u64 },
    OutputRowsExceeded { rows: u64, limit: u64 },
    OutputTextExceeded { bytes: u64, limit: u64 },
}

/// exact name text and original location/coordinates, independent of input lifetime.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeOwnedBoundImportName {
    pub location: PeBoundImportNameLocation,
    pub name_rva: RelativeVirtualAddress,
    pub name_file_offset: FileOffset,
    pub dll_name: String,
}

/// complete raw table and ordered owned names, including duplicate occurrences.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeOwnedBoundImportNameTable {
    pub table: PeBoundImportTable,
    pub names: Vec<PeOwnedBoundImportName>,
}

/// independent reader outcomes that outlive input; absence and empty stay distinct.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeBoundImportEvidence {
    pub total_rows: u64,
    pub total_text_bytes: u64,
    pub descriptors: Result<Option<PeBoundImportTable>, PeBoundImportError>,
    pub names: Result<Option<PeOwnedBoundImportNameTable>, PeBoundImportNameError>,
}

fn table_rows(table: &PeBoundImportTable) -> u64 {
    table
        .descriptors
        .iter()
        .map(|descriptor| 1 + descriptor.forwarder_refs.len() as u64)
        .sum()
}

fn own_names(table: PeBoundImportNameTable<'_>) -> PeOwnedBoundImportNameTable {
    PeOwnedBoundImportNameTable {
        table: table.table,
        names: table
            .names
            .into_iter()
            .map(|name| PeOwnedBoundImportName {
                location: name.location,
                name_rva: name.name_rva,
                name_file_offset: name.name_file_offset,
                dll_name: name.dll_name.to_owned(),
            })
            .collect(),
    }
}

/// collects independent owned bound-import raw and name results from one input.
/// input admission precedes both readers. each successful standalone descriptor
/// and reference, nested descriptor and reference inside names, and explicit name
/// entry costs one row. fixed wrappers and directory/terminator metadata cost none.
/// each copied name occurrence costs its byte length, excluding nul; duplicate
/// and overlapping strings count each time. absent and failed results cost zero.
/// present-empty tables retain metadata with zero output caps.
///
/// complete row admission precedes text admission and new owned name conversion
/// copies. existing reader allocations, including both already-owned raw tables,
/// occur earlier; those tables move into the evidence. reader caps bound output
/// to 3456 rows and fewer than 65536 text bytes. these logical limits do not cap
/// process memory, resolver work or execution time, do not recover allocation
/// failure, and offer no cancellation.
///
/// a name error preserves readable standalone raw records; discarded name prefixes
/// contribute no output. errors, coordinates, order, spelling and raw fields stay
/// unchanged. public-field mutation can break the original record/name pairing.
/// this does not validate paths, select providers or establish binding validity.
///
/// ```
/// use ring3_core::{PeBoundImportEvidenceLimits, inspect_pe_bound_imports};
/// let evidence = {
///     let bytes = Vec::new();
///     inspect_pe_bound_imports(&bytes, PeBoundImportEvidenceLimits {
///         max_input_bytes: 0, max_output_rows: 0, max_output_text_bytes: 0,
///     }).unwrap()
/// };
/// assert!(evidence.descriptors.is_err());
/// assert!(evidence.names.is_err());
/// assert_eq!((evidence.total_rows, evidence.total_text_bytes), (0, 0));
/// ```
///
/// # errors
/// refuses input bytes, then complete output rows, then complete output text.
/// exact limits succeed; outer refusal retains actual totals and returns no
/// partial evidence. complete typed reader errors remain nested on success.
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn inspect_pe_bound_imports(
    bytes: &[u8],
    limits: PeBoundImportEvidenceLimits,
) -> Result<PeBoundImportEvidence, PeBoundImportEvidenceError> {
    const {
        assert!(usize::BITS <= 64);
    }
    let length = bytes.len() as u64;
    if length > limits.max_input_bytes {
        return Err(PeBoundImportEvidenceError::InputTooLarge {
            length,
            limit: limits.max_input_bytes,
        });
    }
    let descriptors = parse_pe_bound_import_descriptors(bytes);
    let names = parse_pe_bound_import_names(bytes);
    let mut rows = 0;
    let mut text_bytes = 0;
    if let Ok(Some(table)) = &descriptors {
        rows += table_rows(table);
    }
    if let Ok(Some(table)) = &names {
        rows += table_rows(&table.table) + table.names.len() as u64;
        for name in &table.names {
            text_bytes += name.dll_name.len() as u64;
        }
    }
    if rows > limits.max_output_rows {
        return Err(PeBoundImportEvidenceError::OutputRowsExceeded {
            rows,
            limit: limits.max_output_rows,
        });
    }
    if text_bytes > limits.max_output_text_bytes {
        return Err(PeBoundImportEvidenceError::OutputTextExceeded {
            bytes: text_bytes,
            limit: limits.max_output_text_bytes,
        });
    }
    Ok(PeBoundImportEvidence {
        total_rows: rows,
        total_text_bytes: text_bytes,
        descriptors,
        names: names.map(|table| table.map(own_names)),
    })
}
