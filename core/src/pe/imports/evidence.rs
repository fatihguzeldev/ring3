use super::{
    PeImportDescriptor, PeImportError, PeImportLookup, PeImportLookupEntry, PeImportLookupError,
    PeImportSymbol,
};
use crate::pe::rva::PreparedPe;
use crate::{FileOffset, RelativeVirtualAddress};

/// per-call input and logical owned-output caps; not allocation or memory limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeStaticImportEvidenceLimits {
    pub max_input_bytes: u64,
    pub max_output_rows: u64,
    pub max_output_text_bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeStaticImportEvidenceError {
    InputTooLarge { length: u64, limit: u64 },
    OutputRowsExceeded { rows: u64, limit: u64 },
    OutputTextExceeded { bytes: u64, limit: u64 },
}

/// exact raw descriptor metadata with owned dll text; no new name coordinates.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeOwnedImportDescriptor {
    pub descriptor_rva: RelativeVirtualAddress,
    pub descriptor_file_offset: FileOffset,
    pub import_lookup_table_rva: RelativeVirtualAddress,
    pub time_date_stamp: u32,
    pub forwarder_chain: u32,
    pub name_rva: RelativeVirtualAddress,
    pub import_address_table_rva: RelativeVirtualAddress,
    pub dll_name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PeOwnedImportSymbol {
    Ordinal(u16),
    ByName {
        hint_name_rva: RelativeVirtualAddress,
        hint: u16,
        name: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeOwnedImportLookupEntry {
    pub lookup_rva: RelativeVirtualAddress,
    pub lookup_file_offset: FileOffset,
    pub raw_value: u64,
    pub symbol: PeOwnedImportSymbol,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeOwnedImportLookup {
    pub descriptor: PeOwnedImportDescriptor,
    pub entries: Vec<PeOwnedImportLookupEntry>,
}

/// independent reader outcomes that outlive input bytes; failures cost no output.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeStaticImportEvidence {
    pub total_rows: u64,
    pub total_text_bytes: u64,
    pub descriptors: Result<Vec<PeOwnedImportDescriptor>, PeImportError>,
    pub lookups: Result<Vec<PeOwnedImportLookup>, PeImportLookupError>,
}

fn own_descriptor(d: PeImportDescriptor<'_>) -> PeOwnedImportDescriptor {
    PeOwnedImportDescriptor {
        descriptor_rva: d.descriptor_rva,
        descriptor_file_offset: d.descriptor_file_offset,
        import_lookup_table_rva: d.import_lookup_table_rva,
        time_date_stamp: d.time_date_stamp,
        forwarder_chain: d.forwarder_chain,
        name_rva: d.name_rva,
        import_address_table_rva: d.import_address_table_rva,
        dll_name: d.dll_name.to_owned(),
    }
}

pub(super) fn own_entry(e: PeImportLookupEntry<'_>) -> PeOwnedImportLookupEntry {
    PeOwnedImportLookupEntry {
        lookup_rva: e.lookup_rva,
        lookup_file_offset: e.lookup_file_offset,
        raw_value: e.raw_value,
        symbol: match e.symbol {
            PeImportSymbol::Ordinal(v) => PeOwnedImportSymbol::Ordinal(v),
            PeImportSymbol::ByName {
                hint_name_rva,
                hint,
                name,
            } => PeOwnedImportSymbol::ByName {
                hint_name_rva,
                hint,
                name: name.to_owned(),
            },
        },
    }
}

fn own_lookup(l: PeImportLookup<'_>) -> PeOwnedImportLookup {
    PeOwnedImportLookup {
        descriptor: own_descriptor(l.descriptor),
        entries: l.entries.into_iter().map(own_entry).collect(),
    }
}

/// collects independent owned static descriptor and lookup results from one input.
/// input admission precedes parsing. same-call prepared input and descriptors are
/// reused for lookup traversal. only successful results contribute to
/// output budgets: each standalone descriptor, lookup descriptor and lookup entry
/// costs one row. every copied dll/symbol text occurrence costs its byte length,
/// excluding nul; duplicates count each time. complete row admission precedes
/// text admission, and both finish before any owned copies. reader allocations
/// keep their existing bounds; these logical limits do not cap process memory.
///
/// errors inside the evidence retain each reader's complete typed outcome.
/// a lookup failure preserves readable descriptors; no partial reader list or
/// iat fallback is added. absent and empty tables retain the readers' empty result.
/// all existing coordinates, raw values, order and ascii bytes are preserved.
/// this does not select providers, infer requirements or establish loadability.
///
/// ```
/// use ring3_core::{PeStaticImportEvidenceLimits, inspect_pe_static_imports};
///
/// let evidence = {
///     let bytes = Vec::new();
///     inspect_pe_static_imports(&bytes, PeStaticImportEvidenceLimits {
///         max_input_bytes: 0,
///         max_output_rows: 0,
///         max_output_text_bytes: 0,
///     }).unwrap()
/// };
/// assert!(evidence.descriptors.is_err());
/// assert!(evidence.lookups.is_err());
/// assert_eq!(evidence.total_rows, 0);
/// ```
///
/// # errors
/// refuses input bytes, then total output rows, then total output text bytes.
/// exact limits succeed; refusal includes the actual total and returns no owned
/// partial result. output totals exclude failed results and discarded prefixes.
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn inspect_pe_static_imports(
    bytes: &[u8],
    limits: PeStaticImportEvidenceLimits,
) -> Result<PeStaticImportEvidence, PeStaticImportEvidenceError> {
    const {
        assert!(usize::BITS <= 64);
    }
    let length = bytes.len() as u64;
    if length > limits.max_input_bytes {
        return Err(PeStaticImportEvidenceError::InputTooLarge {
            length,
            limit: limits.max_input_bytes,
        });
    }
    let admitted = PreparedPe::new(bytes)
        .map_err(PeImportError::Base)
        .and_then(|prepared| {
            super::descriptors::parse_prepared_descriptors(&prepared)
                .map(|descriptors| (prepared, descriptors))
        });
    let (descriptors, lookups) = match admitted {
        Ok((prepared, descriptors)) => {
            let lookups = super::lookups::parse_admitted_lookups(&prepared, &descriptors);
            (Ok(descriptors), lookups)
        }
        Err(cause) => (Err(cause), Err(PeImportLookupError::Descriptors(cause))),
    };
    let mut rows = 0_u64;
    let mut text_bytes = 0_u64;
    // reader limits bound totals to 4352 rows and fewer than 196608 text bytes.
    if let Ok(ds) = &descriptors {
        rows += ds.len() as u64;
        for d in ds {
            text_bytes += d.dll_name.len() as u64;
        }
    }
    if let Ok(ls) = &lookups {
        rows += ls.len() as u64;
        for l in ls {
            rows += l.entries.len() as u64;
            text_bytes += l.descriptor.dll_name.len() as u64;
            for e in &l.entries {
                if let PeImportSymbol::ByName { name, .. } = e.symbol {
                    text_bytes += name.len() as u64;
                }
            }
        }
    }
    if rows > limits.max_output_rows {
        return Err(PeStaticImportEvidenceError::OutputRowsExceeded {
            rows,
            limit: limits.max_output_rows,
        });
    }
    if text_bytes > limits.max_output_text_bytes {
        return Err(PeStaticImportEvidenceError::OutputTextExceeded {
            bytes: text_bytes,
            limit: limits.max_output_text_bytes,
        });
    }
    Ok(PeStaticImportEvidence {
        total_rows: rows,
        total_text_bytes: text_bytes,
        descriptors: descriptors.map(|ds| ds.into_iter().map(own_descriptor).collect()),
        lookups: lookups.map(|ls| ls.into_iter().map(own_lookup).collect()),
    })
}
