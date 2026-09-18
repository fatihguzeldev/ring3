use super::super::evidence::own_entry;
use super::{
    PeDelayImportDescriptor, PeDelayImportError, PeDelayImportLookupError,
    PeDelayImportLookupTable, PeDelayImportName, PeDelayImportNameError, PeDelayImportNameTable,
    PeDelayImportTable,
};
use crate::pe::{PeRvaError, rva::PreparedPe};
use crate::{FileOffset, PeImportSymbol, PeKind, PeOwnedImportLookupEntry, RelativeVirtualAddress};

/// per-call input and logical output caps; not allocation or memory limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeDelayImportEvidenceLimits {
    pub max_input_bytes: u64,
    pub max_output_rows: u64,
    pub max_output_text_bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeDelayImportEvidenceError {
    InputTooLarge { length: u64, limit: u64 },
    OutputRowsExceeded { rows: u64, limit: u64 },
    OutputTextExceeded { bytes: u64, limit: u64 },
}

/// exact raw descriptor metadata with owned dll text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeOwnedDelayImportName {
    pub descriptor: PeDelayImportDescriptor,
    pub dll_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeOwnedDelayImportNameTable {
    pub kind: PeKind,
    pub directory_rva: RelativeVirtualAddress,
    pub directory_file_offset: FileOffset,
    pub directory_size: u32,
    pub imports: Vec<PeOwnedDelayImportName>,
    pub terminator_rva: RelativeVirtualAddress,
    pub terminator_file_offset: FileOffset,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeOwnedDelayImportLookup {
    pub import: PeOwnedDelayImportName,
    pub entries: Vec<PeOwnedImportLookupEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeOwnedDelayImportLookupTable {
    pub kind: PeKind,
    pub directory_rva: RelativeVirtualAddress,
    pub directory_file_offset: FileOffset,
    pub directory_size: u32,
    pub imports: Vec<PeOwnedDelayImportLookup>,
    pub terminator_rva: RelativeVirtualAddress,
    pub terminator_file_offset: FileOffset,
}

/// independent reader outcomes that outlive input; absence and empty stay distinct.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeDelayImportEvidence {
    pub total_rows: u64,
    pub total_text_bytes: u64,
    pub descriptors: Result<Option<PeDelayImportTable>, PeDelayImportError>,
    pub names: Result<Option<PeOwnedDelayImportNameTable>, PeDelayImportNameError>,
    pub lookups: Result<Option<PeOwnedDelayImportLookupTable>, PeDelayImportLookupError>,
}

fn own_name(n: PeDelayImportName<'_>) -> PeOwnedDelayImportName {
    PeOwnedDelayImportName {
        descriptor: n.descriptor,
        dll_name: n.dll_name.to_owned(),
    }
}

fn own_names(t: PeDelayImportNameTable<'_>) -> PeOwnedDelayImportNameTable {
    PeOwnedDelayImportNameTable {
        kind: t.kind,
        directory_rva: t.directory_rva,
        directory_file_offset: t.directory_file_offset,
        directory_size: t.directory_size,
        imports: t.imports.into_iter().map(own_name).collect(),
        terminator_rva: t.terminator_rva,
        terminator_file_offset: t.terminator_file_offset,
    }
}

fn own_lookups(t: PeDelayImportLookupTable<'_>) -> PeOwnedDelayImportLookupTable {
    PeOwnedDelayImportLookupTable {
        kind: t.kind,
        directory_rva: t.directory_rva,
        directory_file_offset: t.directory_file_offset,
        directory_size: t.directory_size,
        imports: t
            .imports
            .into_iter()
            .map(|l| PeOwnedDelayImportLookup {
                import: own_name(l.import),
                entries: l.entries.into_iter().map(own_entry).collect(),
            })
            .collect(),
        terminator_rva: t.terminator_rva,
        terminator_file_offset: t.terminator_file_offset,
    }
}

/// collects independent owned delay descriptor, name and lookup results.
/// input admission precedes parsing. same-call prepared input, raw descriptors and
/// names are reused by their dependent views. each successful raw descriptor,
/// named descriptor, lookup descriptor and lookup entry costs one output row.
/// table wrappers and directory/terminator metadata cost no rows. each copied
/// dll/symbol text occurrence costs its byte length, excluding nul; duplicates
/// count each time. absent and failed results cost zero output. present-empty
/// tables retain their metadata even with zero row and text limits.
///
/// complete row admission precedes text admission and new owned text/conversion
/// copies. existing reader allocations, including the already-owned raw table,
/// occur earlier and keep their bounds; the raw result moves into the evidence.
/// these logical limits do not cap process memory or recover allocation failure.
///
/// each reader's exact typed result survives independently. unsupported attributes
/// remain raw words; later name/lookup failures preserve earlier readable views.
/// no iat fallback, address conversion, normalization or new coordinates are added.
/// order, duplicates, ascii bytes and all existing metadata remain unchanged.
/// this does not select providers, infer requirements or establish loadability.
///
/// ```
/// use ring3_core::{PeDelayImportEvidenceLimits, inspect_pe_delay_imports};
///
/// let evidence = {
///     let bytes = Vec::new();
///     inspect_pe_delay_imports(&bytes, PeDelayImportEvidenceLimits {
///         max_input_bytes: 0,
///         max_output_rows: 0,
///         max_output_text_bytes: 0,
///     }).unwrap()
/// };
/// assert!(evidence.descriptors.is_err());
/// assert!(evidence.names.is_err());
/// assert!(evidence.lookups.is_err());
/// assert_eq!(evidence.total_rows, 0);
/// ```
///
/// # errors
/// refuses input bytes, then complete output rows, then complete output text.
/// exact limits succeed; outer refusal includes the actual total and returns
/// no partial evidence. failed reader results and discarded prefixes cost zero.
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn inspect_pe_delay_imports(
    bytes: &[u8],
    limits: PeDelayImportEvidenceLimits,
) -> Result<PeDelayImportEvidence, PeDelayImportEvidenceError> {
    const {
        assert!(usize::BITS <= 64);
    }
    let length = bytes.len() as u64;
    if length > limits.max_input_bytes {
        return Err(PeDelayImportEvidenceError::InputTooLarge {
            length,
            limit: limits.max_input_bytes,
        });
    }
    let prepared = PreparedPe::new(bytes);
    inspect_prepared_delay_imports(prepared.as_ref().map_err(|cause| *cause), limits)
}

// the caller has admitted this same input before preparing it.
pub(in crate::pe) fn inspect_prepared_delay_imports(
    prepared: Result<&PreparedPe<'_>, PeRvaError>,
    limits: PeDelayImportEvidenceLimits,
) -> Result<PeDelayImportEvidence, PeDelayImportEvidenceError> {
    let admitted = prepared
        .map_err(PeDelayImportError::Base)
        .and_then(|prepared| {
            super::descriptors::parse_prepared_descriptors(prepared).map(|raw| (prepared, raw))
        });
    let (raw, names, lookups) = match admitted {
        Ok((prepared, raw)) => {
            let names = raw
                .as_ref()
                .map(|table| super::names::parse_admitted_names(prepared, table))
                .transpose();
            let lookups = match &names {
                Ok(table) => table
                    .as_ref()
                    .map(|table| super::lookups::parse_admitted_lookups(prepared, table))
                    .transpose(),
                Err(cause) => Err(PeDelayImportLookupError::Names(*cause)),
            };
            (Ok(raw), names, lookups)
        }
        Err(cause) => (
            Err(cause),
            Err(PeDelayImportNameError::Table(cause)),
            Err(PeDelayImportLookupError::Names(
                PeDelayImportNameError::Table(cause),
            )),
        ),
    };
    let mut rows = 0_u64;
    let mut text_bytes = 0_u64;
    // reader bounds limit the combined views to 4480 rows and less than 196608 text bytes.
    if let Ok(Some(t)) = &raw {
        rows += t.descriptors.len() as u64;
    }
    if let Ok(Some(t)) = &names {
        rows += t.imports.len() as u64;
        for n in &t.imports {
            text_bytes += n.dll_name.len() as u64;
        }
    }
    if let Ok(Some(t)) = &lookups {
        rows += t.imports.len() as u64;
        for l in &t.imports {
            rows += l.entries.len() as u64;
            text_bytes += l.import.dll_name.len() as u64;
            for e in &l.entries {
                if let PeImportSymbol::ByName { name, .. } = e.symbol {
                    text_bytes += name.len() as u64;
                }
            }
        }
    }
    if rows > limits.max_output_rows {
        return Err(PeDelayImportEvidenceError::OutputRowsExceeded {
            rows,
            limit: limits.max_output_rows,
        });
    }
    if text_bytes > limits.max_output_text_bytes {
        return Err(PeDelayImportEvidenceError::OutputTextExceeded {
            bytes: text_bytes,
            limit: limits.max_output_text_bytes,
        });
    }
    Ok(PeDelayImportEvidence {
        total_rows: rows,
        total_text_bytes: text_bytes,
        descriptors: raw,
        names: names.map(|t| t.map(own_names)),
        lookups: lookups.map(|t| t.map(own_lookups)),
    })
}
