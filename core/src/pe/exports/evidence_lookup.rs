use super::{
    PeExportAddressEntry, PeExportEvidence, PeExportLookupError, PeExportName, PeExportQuery,
    PeExportSelection, PeExportTarget, PeOwnedExportAddressEntry, PeOwnedExportAddressTable,
    PeOwnedExportName, PeOwnedExportNameTable, PeOwnedExportTarget,
};

/// per-query logical rows and text in the required retained view.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeExportEvidenceLookupLimits {
    pub max_table_rows: u64,
    pub max_table_text_bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeExportEvidenceLookupError {
    Reader(PeExportLookupError),
    RowsOverflow {
        address_rows: u64,
        name_rows: u64,
    },
    RowsExceeded {
        rows: u64,
        limit: u64,
    },
    TextOverflow {
        row_index: u64,
        total: u64,
        bytes: u64,
    },
    TextExceeded {
        bytes: u64,
        limit: u64,
    },
    AddressCountMismatch {
        declared: u32,
        actual: u64,
    },
    AddressIndexMismatch {
        entry_index: u32,
        table_index: u32,
    },
    OrdinalOverflow {
        ordinal_base: u32,
        entry_index: u32,
    },
    OrdinalMismatch {
        entry_index: u32,
        expected: u32,
        actual: u32,
    },
    NameCountMismatch {
        declared: u32,
        actual: u64,
    },
    NameIndexMismatch {
        entry_index: u32,
        table_index: u32,
    },
    NameAddressIndexOutOfRange {
        entry_index: u32,
        address_index: u16,
        address_count: u32,
    },
}

fn address(entry: &PeOwnedExportAddressEntry) -> PeExportAddressEntry<'_> {
    PeExportAddressEntry {
        table_index: entry.table_index,
        ordinal: entry.ordinal,
        entry_rva: entry.entry_rva,
        entry_file_offset: entry.entry_file_offset,
        target: match &entry.target {
            PeOwnedExportTarget::Empty => PeExportTarget::Empty,
            PeOwnedExportTarget::Rva(rva) => PeExportTarget::Rva(*rva),
            PeOwnedExportTarget::Forwarder { rva, text } => {
                PeExportTarget::Forwarder { rva: *rva, text }
            }
        },
    }
}

fn name(entry: &PeOwnedExportName) -> PeExportName<'_> {
    PeExportName {
        table_index: entry.table_index,
        name_pointer_rva: entry.name_pointer_rva,
        name_pointer_file_offset: entry.name_pointer_file_offset,
        ordinal_entry_rva: entry.ordinal_entry_rva,
        ordinal_entry_file_offset: entry.ordinal_entry_file_offset,
        address_index: entry.address_index,
        name_rva: entry.name_rva,
        name_file_offset: entry.name_file_offset,
        name: &entry.name,
    }
}

fn admit_rows(
    address_rows: u64,
    name_rows: u64,
    limit: u64,
) -> Result<(), PeExportEvidenceLookupError> {
    let rows =
        address_rows
            .checked_add(name_rows)
            .ok_or(PeExportEvidenceLookupError::RowsOverflow {
                address_rows,
                name_rows,
            })?;
    if rows > limit {
        return Err(PeExportEvidenceLookupError::RowsExceeded { rows, limit });
    }
    Ok(())
}
fn charge_text(total: u64, row_index: u64, bytes: u64) -> Result<u64, PeExportEvidenceLookupError> {
    total
        .checked_add(bytes)
        .ok_or(PeExportEvidenceLookupError::TextOverflow {
            row_index,
            total,
            bytes,
        })
}
fn measure_text(
    addresses: impl IntoIterator<Item = u64>,
    names: impl IntoIterator<Item = u64>,
    limit: u64,
) -> Result<(), PeExportEvidenceLookupError> {
    let mut total = 0;
    for (row_index, bytes) in (0_u64..).zip(addresses.into_iter().chain(names)) {
        total = charge_text(total, row_index, bytes)?;
    }
    if total > limit {
        return Err(PeExportEvidenceLookupError::TextExceeded {
            bytes: total,
            limit,
        });
    }
    Ok(())
}
fn admit(
    addresses: &PeOwnedExportAddressTable,
    names: &[PeOwnedExportName],
    limits: PeExportEvidenceLookupLimits,
) -> Result<(), PeExportEvidenceLookupError> {
    admit_rows(
        addresses.entries.len() as u64,
        names.len() as u64,
        limits.max_table_rows,
    )?;
    measure_text(
        addresses.entries.iter().map(|entry| match &entry.target {
            PeOwnedExportTarget::Forwarder { text, .. } => text.len() as u64,
            _ => 0,
        }),
        names.iter().map(|entry| entry.name.len() as u64),
        limits.max_table_text_bytes,
    )?;
    let actual = addresses.entries.len() as u64;
    let declared = addresses.directory.address_table_entries;
    if u64::from(declared) != actual {
        return Err(PeExportEvidenceLookupError::AddressCountMismatch { declared, actual });
    }
    for (entry_index, entry) in (0..declared).zip(&addresses.entries) {
        if entry.table_index != entry_index {
            return Err(PeExportEvidenceLookupError::AddressIndexMismatch {
                entry_index,
                table_index: entry.table_index,
            });
        }
        let ordinal_base = addresses.directory.ordinal_base;
        let expected = ordinal_base.checked_add(entry.table_index).ok_or(
            PeExportEvidenceLookupError::OrdinalOverflow {
                ordinal_base,
                entry_index,
            },
        )?;
        if entry.ordinal != expected {
            return Err(PeExportEvidenceLookupError::OrdinalMismatch {
                entry_index,
                expected,
                actual: entry.ordinal,
            });
        }
    }
    Ok(())
}
fn admit_names(table: &PeOwnedExportNameTable) -> Result<(), PeExportEvidenceLookupError> {
    let declared = table.addresses.directory.number_of_name_pointers;
    let actual = table.entries.len() as u64;
    if u64::from(declared) != actual {
        return Err(PeExportEvidenceLookupError::NameCountMismatch { declared, actual });
    }
    let address_count = table.addresses.directory.address_table_entries;
    for (entry_index, entry) in (0..declared).zip(&table.entries) {
        if entry.table_index != entry_index {
            return Err(PeExportEvidenceLookupError::NameIndexMismatch {
                entry_index,
                table_index: entry.table_index,
            });
        }
        if u32::from(entry.address_index) >= address_count {
            return Err(PeExportEvidenceLookupError::NameAddressIndexOutOfRange {
                entry_index,
                address_index: entry.address_index,
                address_count,
            });
        }
    }
    Ok(())
}
/// queries the required owned export view after actual row/text admission.
/// name queries use names and their nested addresses; ordinal queries use the
/// standalone address table. top-level directory, unrelated views and reported
/// totals are ignored. caller-built views may disagree; no coherence or image
/// provenance is established.
///
/// stored reader errors and absent views return before budgets. present views
/// admit actual address rows plus name rows when required, then complete text
/// bytes, then the entire required structure, before matching or allocation.
/// fixed wrappers cost no rows; every retained forwarder/name string occurrence
/// costs its byte length. zero-text address rows still occupy text-error indices;
/// name indices follow all address rows. checked overflow precedes the respective
/// limit error, and text limits report the complete total. exact caps pass.
///
/// address count, positional index and checked base-plus-index ordinal are
/// validated first; name count, positional index and address references follow.
/// even late invalid rows refuse an early match or range shortcut. names compare
/// exactly and duplicate matches retain their order, including repeated address
/// indices. ordinal selections retain no name. raw targets, coordinates and
/// arbitrary utf-8/empty text are preserved without pe revalidation or decoding.
///
/// results borrow evidence, independently of query lifetime. ambiguity contains
/// at most admitted name rows and borrows strings. limits do not bound existing
/// caller allocations, vector capacity, allocator failure, time or cancellation.
/// admission repeats for every call; there is no aggregate query budget.
///
/// ```
/// use ring3_core::{PeExportEvidence, PeExportEvidenceLookupLimits, PeExportQuery,
///     PeExportSelection, lookup_pe_export_evidence};
/// let evidence = PeExportEvidence {
///     total_rows: u64::MAX, total_text_bytes: u64::MAX,
///     directory: Ok(None), addresses: Ok(None), names: Ok(None),
/// };
/// let result = {
///     let query = String::from("example");
///     lookup_pe_export_evidence(&evidence, PeExportQuery::Name(&query),
///         PeExportEvidenceLookupLimits { max_table_rows: 0, max_table_text_bytes: 0 })
/// };
/// assert_eq!(result, Ok(PeExportSelection::DirectoryAbsent));
/// ```
///
/// ```compile_fail,E0515
/// use ring3_core::{PeExportEvidence, PeExportEvidenceLookupError,
///     PeExportEvidenceLookupLimits, PeExportQuery, PeExportSelection,
///     lookup_pe_export_evidence};
/// fn escape() -> Result<PeExportSelection<'static>, PeExportEvidenceLookupError> {
///     let evidence = PeExportEvidence {
///         total_rows: 0, total_text_bytes: 0,
///         directory: Ok(None), addresses: Ok(None), names: Ok(None),
///     };
///     lookup_pe_export_evidence(&evidence, PeExportQuery::Ordinal(1),
///         PeExportEvidenceLookupLimits { max_table_rows: 0, max_table_text_bytes: 0 })
/// }
/// ```
///
/// ```compile_fail,E0506
/// use ring3_core::{PeExportEvidence, PeExportEvidenceLookupError,
///     PeExportEvidenceLookupLimits, PeExportQuery, PeExportSelection,
///     lookup_pe_export_evidence};
/// fn mutate(evidence: &mut PeExportEvidence)
///     -> Result<PeExportSelection<'_>, PeExportEvidenceLookupError> {
///     let result = lookup_pe_export_evidence(evidence, PeExportQuery::Ordinal(1),
///         PeExportEvidenceLookupLimits { max_table_rows: 0, max_table_text_bytes: 0 });
///     evidence.addresses = Ok(None);
///     result
/// }
/// ```
///
/// # errors
/// returns the required reader error, row/text overflow or limit error, then
/// the first structural mismatch in the order described above. no partial
/// selection is returned on refusal.
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn lookup_pe_export_evidence<'e>(
    evidence: &'e PeExportEvidence,
    query: PeExportQuery<'_>,
    limits: PeExportEvidenceLookupLimits,
) -> Result<PeExportSelection<'e>, PeExportEvidenceLookupError> {
    EvidenceLookup::new(evidence, limits).lookup(query)
}

// caller-built name and ordinal views are independent; admission cannot be shared.
pub(super) struct EvidenceLookup<'e> {
    evidence: &'e PeExportEvidence,
    limits: PeExportEvidenceLookupLimits,
    names: Option<Result<Option<&'e PeOwnedExportNameTable>, PeExportEvidenceLookupError>>,
    addresses: Option<Result<Option<&'e PeOwnedExportAddressTable>, PeExportEvidenceLookupError>>,
}

impl<'e> EvidenceLookup<'e> {
    pub(super) fn new(
        evidence: &'e PeExportEvidence,
        limits: PeExportEvidenceLookupLimits,
    ) -> Self {
        Self {
            evidence,
            limits,
            names: None,
            addresses: None,
        }
    }

    fn names(&mut self) -> Result<Option<&'e PeOwnedExportNameTable>, PeExportEvidenceLookupError> {
        *self.names.get_or_insert_with(|| {
            let Some(table) =
                self.evidence.names.as_ref().map_err(|e| {
                    PeExportEvidenceLookupError::Reader(PeExportLookupError::Names(*e))
                })?
            else {
                return Ok(None);
            };
            admit(&table.addresses, &table.entries, self.limits)?;
            admit_names(table)?;
            Ok(Some(table))
        })
    }

    fn addresses(
        &mut self,
    ) -> Result<Option<&'e PeOwnedExportAddressTable>, PeExportEvidenceLookupError> {
        *self.addresses.get_or_insert_with(|| {
            let Some(table) = self.evidence.addresses.as_ref().map_err(|e| {
                PeExportEvidenceLookupError::Reader(PeExportLookupError::Addresses(*e))
            })?
            else {
                return Ok(None);
            };
            admit(table, &[], self.limits)?;
            Ok(Some(table))
        })
    }

    pub(super) fn lookup(
        &mut self,
        query: PeExportQuery<'_>,
    ) -> Result<PeExportSelection<'e>, PeExportEvidenceLookupError> {
        match query {
            PeExportQuery::Name(query) => {
                let Some(table) = self.names()? else {
                    return Ok(PeExportSelection::DirectoryAbsent);
                };
                let matches: Vec<_> = table
                    .entries
                    .iter()
                    .filter(|e| e.name == query)
                    .map(name)
                    .collect();
                Ok(match matches.as_slice() {
                    [] => PeExportSelection::NameNotFound,
                    [name] => {
                        let entry = table
                            .addresses
                            .entries
                            .get(usize::from(name.address_index))
                            .ok_or(PeExportEvidenceLookupError::NameAddressIndexOutOfRange {
                                entry_index: name.table_index,
                                address_index: name.address_index,
                                address_count: table.addresses.directory.address_table_entries,
                            })?;
                        PeExportSelection::Selected {
                            address: address(entry),
                            name: Some(*name),
                        }
                    }
                    _ => PeExportSelection::AmbiguousName { matches },
                })
            }
            PeExportQuery::Ordinal(ordinal) => {
                let Some(table) = self.addresses()? else {
                    return Ok(PeExportSelection::DirectoryAbsent);
                };
                let base = table.directory.ordinal_base;
                let Some(index) = ordinal.checked_sub(base) else {
                    return Ok(PeExportSelection::OrdinalBeforeBase { ordinal, base });
                };
                let Some(entry) = usize::try_from(index)
                    .ok()
                    .and_then(|i| table.entries.get(i))
                else {
                    return Ok(PeExportSelection::OrdinalOutOfRange {
                        ordinal,
                        base,
                        address_count: table.directory.address_table_entries,
                    });
                };
                Ok(PeExportSelection::Selected {
                    address: address(entry),
                    name: None,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn row_overflow_precedes_limit_with_complete_operands() {
        assert_eq!(
            admit_rows(u64::MAX, 1, 0),
            Err(PeExportEvidenceLookupError::RowsOverflow {
                address_rows: u64::MAX,
                name_rows: 1
            })
        );
        assert_eq!(admit_rows(u64::MAX, 0, u64::MAX), Ok(()));
    }
    #[test]
    fn text_overflow_preserves_attempted_charge_before_limit() {
        assert_eq!(
            charge_text(u64::MAX - 1, 5, 2),
            Err(PeExportEvidenceLookupError::TextOverflow {
                row_index: 5,
                total: u64::MAX - 1,
                bytes: 2
            })
        );
        assert_eq!(charge_text(u64::MAX - 1, 5, 1), Ok(u64::MAX));
    }
    #[test]
    fn name_text_overflow_indices_include_every_address_row() {
        assert_eq!(
            measure_text([u64::MAX - 1, 0, 0, 0, 0, 0], [0, 0, 0, 2], 0),
            Err(PeExportEvidenceLookupError::TextOverflow {
                row_index: 9,
                total: u64::MAX - 1,
                bytes: 2
            })
        );
        assert_eq!(measure_text([u64::MAX - 1, 0], [1], u64::MAX), Ok(()));
    }
}
