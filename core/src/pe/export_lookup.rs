use super::export_names::parse_prepared_export_name_entries;
use super::rva::PreparedPe;
use super::{
    PeExportAddressEntry, PeExportAddressError, PeExportAddressTable, PeExportDirectoryError,
    PeExportName, PeExportNameError, parse_pe_export_addresses,
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
    PeExportLookup::new(bytes).lookup(query)
}

/// lazy export tables for one immutable image; no global cache or module search.
///
/// query kinds share one retained address result, including absence and errors.
/// name entries stay lazy; a name-entry error does not prevent ordinal selection.
/// existing reader limits apply; retained tables are not a total memory cap.
/// selections borrow image text and may outlive this owner and the query.
///
/// ```
/// use ring3_core::{PeExportLookup, PeExportQuery};
/// let mut lookup = PeExportLookup::new(b"");
/// assert!(lookup.lookup(PeExportQuery::Name("entry")).is_err());
/// assert!(lookup.lookup(PeExportQuery::Ordinal(1)).is_err());
/// ```
///
/// ```compile_fail
/// use ring3_core::PeExportLookup;
/// fn escape() -> PeExportLookup<'static> {
///     let bytes = vec![0; 64];
///     PeExportLookup::new(&bytes)
/// }
/// ```
///
/// ```compile_fail
/// use ring3_core::{PeExportLookup, PeExportQuery};
/// let mut bytes = vec![0; 64];
/// let mut lookup = PeExportLookup::new(&bytes);
/// bytes[0] = 1;
/// lookup.lookup(PeExportQuery::Ordinal(1));
/// ```
pub struct PeExportLookup<'a> {
    bytes: &'a [u8],
    names: Option<Result<Vec<PeExportName<'a>>, PeExportNameError>>,
    addresses: Option<Result<Option<PeExportAddressTable<'a>>, PeExportAddressError>>,
}

impl<'a> PeExportLookup<'a> {
    /// borrows the image without parsing or allocating tables.
    #[must_use]
    pub const fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            names: None,
            addresses: None,
        }
    }

    /// selects exact metadata, reusing each required reader result after first use.
    ///
    /// name queries validate complete names and addresses; ordinal queries validate
    /// only addresses. matching, duplicate order and raw targets follow the same
    /// rules as `lookup_pe_export`. returned text borrows only the image.
    ///
    /// # errors
    /// retains the query-specific typed reader error, independently of other queries.
    #[expect(
        clippy::missing_errors_doc,
        reason = "project documentation headings are lower case"
    )]
    pub fn lookup(
        &mut self,
        query: PeExportQuery<'_>,
    ) -> Result<PeExportSelection<'a>, PeExportLookupError> {
        let addresses = match self
            .addresses
            .get_or_insert_with(|| parse_pe_export_addresses(self.bytes))
        {
            Err(error) => {
                return Err(match query {
                    PeExportQuery::Name(_) => {
                        PeExportLookupError::Names(PeExportNameError::Addresses(*error))
                    }
                    PeExportQuery::Ordinal(_) => PeExportLookupError::Addresses(*error),
                });
            }
            Ok(None) => return Ok(PeExportSelection::DirectoryAbsent),
            Ok(Some(table)) => table,
        };
        match query {
            PeExportQuery::Name(query) => {
                let entries = match self.names.get_or_insert_with(|| {
                    let prepared = PreparedPe::new(self.bytes).map_err(|cause| {
                        PeExportNameError::Addresses(PeExportAddressError::Directory(
                            PeExportDirectoryError::Base(cause),
                        ))
                    })?;
                    parse_prepared_export_name_entries(&prepared, addresses)
                }) {
                    Err(error) => return Err(PeExportLookupError::Names(*error)),
                    Ok(entries) => entries,
                };
                let matches: Vec<_> = entries
                    .iter()
                    .copied()
                    .filter(|entry| entry.name == query)
                    .collect();
                Ok(match matches.as_slice() {
                    [] => PeExportSelection::NameNotFound,
                    [name] => PeExportSelection::Selected {
                        address: addresses.entries[usize::from(name.address_index)],
                        name: Some(*name),
                    },
                    _ => PeExportSelection::AmbiguousName { matches },
                })
            }
            PeExportQuery::Ordinal(ordinal) => {
                let base = addresses.directory.ordinal_base;
                let Some(index) = ordinal.checked_sub(base) else {
                    return Ok(PeExportSelection::OrdinalBeforeBase { ordinal, base });
                };
                let Some(address) = addresses
                    .entries
                    .iter()
                    .find(|entry| entry.table_index == index)
                else {
                    return Ok(PeExportSelection::OrdinalOutOfRange {
                        ordinal,
                        base,
                        address_count: addresses.directory.address_table_entries,
                    });
                };
                Ok(PeExportSelection::Selected {
                    address: *address,
                    name: None,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{PeExportLookup, PeExportLookupError, PeExportNameError, PeExportQuery};

    #[test]
    fn construction_and_query_kinds_share_lazy_address_errors() {
        for name_first in [false, true] {
            let mut lookup = PeExportLookup::new(b"");
            assert!(lookup.names.is_none() && lookup.addresses.is_none());
            let queries = if name_first {
                [PeExportQuery::Name("entry"), PeExportQuery::Ordinal(1)]
            } else {
                [PeExportQuery::Ordinal(1), PeExportQuery::Name("entry")]
            };
            for query in queries {
                let result = lookup.lookup(query);
                let Some(Err(cause)) = lookup.addresses else {
                    panic!()
                };
                assert!(lookup.names.is_none());
                let expected = match query {
                    PeExportQuery::Name(_) => {
                        PeExportLookupError::Names(PeExportNameError::Addresses(cause))
                    }
                    PeExportQuery::Ordinal(_) => PeExportLookupError::Addresses(cause),
                };
                assert_eq!(result, Err(expected));
                assert_eq!(lookup.lookup(query), result);
            }
        }
    }
}
