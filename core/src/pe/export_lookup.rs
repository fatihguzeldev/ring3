use super::{
    PeExportAddressEntry, PeExportAddressError, PeExportName, PeExportNameError,
    parse_pe_export_addresses, parse_pe_export_names,
};

/// exact name or full biased export ordinal; no module search or hint lookup.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeExportQuery<'a> {
    Name(&'a str),
    Ordinal(u32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeExportLookupError {
    Addresses(PeExportAddressError),
    Names(PeExportNameError),
}

/// raw selection metadata, borrowing text only from the supplied image bytes.
///
/// ```compile_fail
/// use ring3_core::{PeExportQuery, PeExportSelection, lookup_pe_export};
/// fn escape() -> PeExportSelection<'static> {
///     let bytes = vec![0; 64];
///     lookup_pe_export(&bytes, PeExportQuery::Ordinal(1)).unwrap()
/// }
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PeExportSelection<'a> {
    DirectoryAbsent,
    NameNotFound,
    /// every matching row in original order, even when address indexes repeat.
    AmbiguousName {
        matches: Vec<PeExportName<'a>>,
    },
    OrdinalBeforeBase {
        ordinal: u32,
        base: u32,
    },
    OrdinalOutOfRange {
        ordinal: u32,
        base: u32,
        address_count: u32,
    },
    /// ordinal queries leave name empty; target metadata is never dereferenced.
    Selected {
        address: PeExportAddressEntry<'a>,
        name: Option<PeExportName<'a>>,
    },
}

/// selects exact export metadata in one image without following forwarders.
///
/// name queries validate the full address and name tables before case-sensitive
/// matching. duplicate rows remain ambiguous. ordinal queries validate the full
/// address table, then subtract its base; they do not parse export names.
/// existing reader limits apply. the result borrows input bytes, not the query.
///
/// # errors
/// preserves the complete typed error from the query's required table reader.
/// a found match or out-of-range query does not hide a malformed required table.
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn lookup_pe_export<'a>(
    bytes: &'a [u8],
    query: PeExportQuery<'_>,
) -> Result<PeExportSelection<'a>, PeExportLookupError> {
    match query {
        PeExportQuery::Name(query) => {
            let Some(table) = parse_pe_export_names(bytes).map_err(PeExportLookupError::Names)?
            else {
                return Ok(PeExportSelection::DirectoryAbsent);
            };
            let matches: Vec<_> = table
                .entries
                .into_iter()
                .filter(|entry| entry.name == query)
                .collect();
            Ok(match matches.as_slice() {
                [] => PeExportSelection::NameNotFound,
                [name] => PeExportSelection::Selected {
                    address: table.addresses.entries[usize::from(name.address_index)],
                    name: Some(*name),
                },
                _ => PeExportSelection::AmbiguousName { matches },
            })
        }
        PeExportQuery::Ordinal(ordinal) => {
            let Some(table) =
                parse_pe_export_addresses(bytes).map_err(PeExportLookupError::Addresses)?
            else {
                return Ok(PeExportSelection::DirectoryAbsent);
            };
            let base = table.directory.ordinal_base;
            let Some(index) = ordinal.checked_sub(base) else {
                return Ok(PeExportSelection::OrdinalBeforeBase { ordinal, base });
            };
            let address_count = table.directory.address_table_entries;
            let Some(address) = table
                .entries
                .into_iter()
                .find(|entry| entry.table_index == index)
            else {
                return Ok(PeExportSelection::OrdinalOutOfRange {
                    ordinal,
                    base,
                    address_count,
                });
            };
            Ok(PeExportSelection::Selected {
                address,
                name: None,
            })
        }
    }
}
