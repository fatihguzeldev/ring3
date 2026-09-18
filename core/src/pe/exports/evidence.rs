use super::{
    PeExportAddressError, PeExportAddressTable, PeExportDirectory, PeExportDirectoryError,
    PeExportNameError, PeExportNameTable, PeExportTarget,
};
use crate::pe::rva::PreparedPe;
use crate::{FileOffset, RelativeVirtualAddress};

/// per-call input and logical output caps; not allocation or memory limits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeExportEvidenceLimits {
    pub max_input_bytes: u64,
    pub max_output_rows: u64,
    pub max_output_text_bytes: u64,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeExportEvidenceError {
    InputTooLarge { length: u64, limit: u64 },
    OutputRowsExceeded { rows: u64, limit: u64 },
    OutputTextExceeded { bytes: u64, limit: u64 },
}
/// exact target classification with owned raw forwarder text; no decoding.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PeOwnedExportTarget {
    Empty,
    Rva(RelativeVirtualAddress),
    Forwarder {
        rva: RelativeVirtualAddress,
        text: String,
    },
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeOwnedExportAddressEntry {
    pub table_index: u32,
    pub ordinal: u32,
    pub entry_rva: RelativeVirtualAddress,
    pub entry_file_offset: FileOffset,
    pub target: PeOwnedExportTarget,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeOwnedExportAddressTable {
    pub directory: PeExportDirectory,
    pub entries: Vec<PeOwnedExportAddressEntry>,
}
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeOwnedExportName {
    pub table_index: u32,
    pub name_pointer_rva: RelativeVirtualAddress,
    pub name_pointer_file_offset: FileOffset,
    pub ordinal_entry_rva: RelativeVirtualAddress,
    pub ordinal_entry_file_offset: FileOffset,
    pub address_index: u16,
    pub name_rva: RelativeVirtualAddress,
    pub name_file_offset: FileOffset,
    pub name: String,
}
/// retains the complete nested address table, including when names are empty.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeOwnedExportNameTable {
    pub addresses: PeOwnedExportAddressTable,
    pub entries: Vec<PeOwnedExportName>,
}
/// independent reader outcomes that outlive input; absence and empty stay distinct.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeExportEvidence {
    pub total_rows: u64,
    pub total_text_bytes: u64,
    pub directory: Result<Option<PeExportDirectory>, PeExportDirectoryError>,
    pub addresses: Result<Option<PeOwnedExportAddressTable>, PeExportAddressError>,
    pub names: Result<Option<PeOwnedExportNameTable>, PeExportNameError>,
}
fn address_size(table: &PeExportAddressTable<'_>) -> (u64, u64) {
    let mut text = 0_u64;
    for e in &table.entries {
        if let PeExportTarget::Forwarder { text: spelling, .. } = e.target {
            text += spelling.len() as u64;
        }
    }
    (table.entries.len() as u64, text)
}
fn own_addresses(table: PeExportAddressTable<'_>) -> PeOwnedExportAddressTable {
    PeOwnedExportAddressTable {
        directory: table.directory,
        entries: table
            .entries
            .into_iter()
            .map(|e| PeOwnedExportAddressEntry {
                table_index: e.table_index,
                ordinal: e.ordinal,
                entry_rva: e.entry_rva,
                entry_file_offset: e.entry_file_offset,
                target: match e.target {
                    PeExportTarget::Empty => PeOwnedExportTarget::Empty,
                    PeExportTarget::Rva(r) => PeOwnedExportTarget::Rva(r),
                    PeExportTarget::Forwarder { rva, text } => PeOwnedExportTarget::Forwarder {
                        rva,
                        text: text.to_owned(),
                    },
                },
            })
            .collect(),
    }
}
fn own_names(table: PeExportNameTable<'_>) -> PeOwnedExportNameTable {
    PeOwnedExportNameTable {
        addresses: own_addresses(table.addresses),
        entries: table
            .entries
            .into_iter()
            .map(|n| PeOwnedExportName {
                table_index: n.table_index,
                name_pointer_rva: n.name_pointer_rva,
                name_pointer_file_offset: n.name_pointer_file_offset,
                ordinal_entry_rva: n.ordinal_entry_rva,
                ordinal_entry_file_offset: n.ordinal_entry_file_offset,
                address_index: n.address_index,
                name_rva: n.name_rva,
                name_file_offset: n.name_file_offset,
                name: n.name.to_owned(),
            })
            .collect(),
    }
}
/// collects independent owned export directory, address and name results.
/// input admission precedes parsing. same-call prepared input, directory and
/// addresses are reused for dependent views. each standalone address entry,
/// nested address entry within names, and name entry costs one output row.
/// fixed directory/table wrappers cost no rows. each forwarder/name text copy
/// costs its byte length, excluding nul; duplicates and nested copies count
/// each time. absent and failed views cost zero. present-empty tables retain
/// metadata with zero output caps; zero names still retain nested addresses.
///
/// complete row admission precedes text admission and new owned text/conversion
/// copies. existing reader allocations occur earlier and retain their bounds.
/// the already-owned directory result moves into the evidence. these logical
/// limits do not cap process memory or recover allocation failure.
///
/// earlier successful views survive later reader errors. all existing fields,
/// coordinates, order, aliases, u32 ordinals and u16 name indices stay unchanged.
/// directory name rva remains unread metadata. raw forwarder text is copied
/// without grammar decoding; direct rva target content is not read. this does
/// not walk forwarders, select providers, infer requirements or prove loadability.
///
/// ```
/// use ring3_core::{PeExportEvidenceLimits, inspect_pe_exports};
///
/// let evidence = {
///     let bytes = Vec::new();
///     inspect_pe_exports(&bytes, PeExportEvidenceLimits {
///         max_input_bytes: 0,
///         max_output_rows: 0,
///         max_output_text_bytes: 0,
///     }).unwrap()
/// };
/// assert!(evidence.directory.is_err());
/// assert!(evidence.addresses.is_err());
/// assert!(evidence.names.is_err());
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
pub fn inspect_pe_exports(
    bytes: &[u8],
    limits: PeExportEvidenceLimits,
) -> Result<PeExportEvidence, PeExportEvidenceError> {
    const {
        assert!(usize::BITS <= 64);
    }
    let length = bytes.len() as u64;
    if length > limits.max_input_bytes {
        return Err(PeExportEvidenceError::InputTooLarge {
            length,
            limit: limits.max_input_bytes,
        });
    }
    let admitted = PreparedPe::new(bytes)
        .map_err(PeExportDirectoryError::Base)
        .and_then(|prepared| {
            super::directory::parse_prepared_export_directory(&prepared)
                .map(|directory| (prepared, directory))
        });
    let (directory, addresses, names) = match admitted {
        Ok((prepared, directory)) => {
            let addresses = directory
                .map(|directory| {
                    super::addresses::parse_admitted_export_addresses(&prepared, directory)
                })
                .transpose();
            let names = match &addresses {
                Ok(table) => table
                    .as_ref()
                    .map(|addresses| {
                        super::names::parse_prepared_export_name_entries(&prepared, addresses).map(
                            |entries| PeExportNameTable {
                                addresses: addresses.clone(),
                                entries,
                            },
                        )
                    })
                    .transpose(),
                Err(cause) => Err(PeExportNameError::Addresses(*cause)),
            };
            (Ok(directory), addresses, names)
        }
        Err(cause) => (
            Err(cause),
            Err(PeExportAddressError::Directory(cause)),
            Err(PeExportNameError::Addresses(
                PeExportAddressError::Directory(cause),
            )),
        ),
    };
    let mut rows = 0_u64;
    let mut text_bytes = 0_u64;
    // reader caps bound standalone/nested address and name views to 12288 rows and less than 196608 text bytes.
    if let Ok(Some(table)) = &addresses {
        let (r, t) = address_size(table);
        rows += r;
        text_bytes += t;
    }
    if let Ok(Some(table)) = &names {
        let (r, t) = address_size(&table.addresses);
        rows += r + table.entries.len() as u64;
        text_bytes += t;
        for n in &table.entries {
            text_bytes += n.name.len() as u64;
        }
    }
    if rows > limits.max_output_rows {
        return Err(PeExportEvidenceError::OutputRowsExceeded {
            rows,
            limit: limits.max_output_rows,
        });
    }
    if text_bytes > limits.max_output_text_bytes {
        return Err(PeExportEvidenceError::OutputTextExceeded {
            bytes: text_bytes,
            limit: limits.max_output_text_bytes,
        });
    }
    Ok(PeExportEvidence {
        total_rows: rows,
        total_text_bytes: text_bytes,
        directory,
        addresses: addresses.map(|t| t.map(own_addresses)),
        names: names.map(|t| t.map(own_names)),
    })
}
