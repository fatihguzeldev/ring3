use super::{PeExportLookup, PeExportLookupError, PeExportQuery, PeExportSelection};

/// caller-selected limits on queries and successful logical selection rows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeExportBatchLimits {
    pub max_queries: u64,
    pub max_selection_rows: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeExportBatchError {
    QueryCountExceeded {
        count: u64,
        limit: u64,
    },
    SelectionRowsOverflow {
        index: usize,
        total: u64,
        rows: u64,
    },
    SelectionRowsExceeded {
        index: usize,
        total: u64,
        limit: u64,
    },
}

/// ordered per-query results; text borrows only the supplied image.
///
/// ```compile_fail
/// use ring3_core::{PeExportBatch, PeExportBatchLimits, PeExportQuery, lookup_pe_export_batch};
/// fn escape() -> PeExportBatch<'static> {
///     let bytes = vec![0; 64];
///     lookup_pe_export_batch(&bytes, &[PeExportQuery::Ordinal(1)],
///         PeExportBatchLimits { max_queries: 1, max_selection_rows: 1 }).unwrap()
/// }
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeExportBatch<'a> {
    pub selection_rows: u64,
    pub selections: Vec<Result<PeExportSelection<'a>, PeExportLookupError>>,
}

/// bounds an ordered query batch while reusing one image's export tables.
///
/// query count is checked before owner creation, result allocation or lookup.
/// empty queries do not inspect the input. each complete query result is charged
/// before append: selected targets cost one row, ambiguity costs every matching
/// row, and other outcomes or per-query errors cost zero. errors stay in order.
///
/// results may outlive the query list and internal owner, but not image bytes.
/// limits bound successful logical output; a rejected query may already have
/// allocated its own result. they do not cap bytes, capacity or process memory.
///
/// # errors
/// count refusal precedes lookup. then each cumulative row addition checks
/// overflow before the row limit. refusal returns no partial batch; the index
/// identifies the offending query, and exceeded total is the attempted total.
///
/// # example
/// ```
/// use ring3_core::{PeExportBatchLimits, PeExportQuery, lookup_pe_export_batch};
/// let batch = lookup_pe_export_batch(b"", &[PeExportQuery::Ordinal(1)],
///     PeExportBatchLimits { max_queries: 1, max_selection_rows: 0 })?;
/// assert_eq!(batch.selection_rows, 0);
/// assert!(batch.selections[0].is_err());
/// # Ok::<(), ring3_core::PeExportBatchError>(())
/// ```
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn lookup_pe_export_batch<'a>(
    bytes: &'a [u8],
    queries: &[PeExportQuery<'_>],
    limits: PeExportBatchLimits,
) -> Result<PeExportBatch<'a>, PeExportBatchError> {
    let count = queries.len() as u64;
    if count > limits.max_queries {
        return Err(PeExportBatchError::QueryCountExceeded {
            count,
            limit: limits.max_queries,
        });
    }
    let mut lookup = PeExportLookup::new(bytes);
    let mut selection_rows = 0;
    let mut selections = Vec::new();
    for (index, &query) in queries.iter().enumerate() {
        let result = lookup.lookup(query);
        let rows = match &result {
            Ok(PeExportSelection::Selected { .. }) => 1,
            Ok(PeExportSelection::AmbiguousName { matches }) => matches.len() as u64,
            Ok(
                PeExportSelection::DirectoryAbsent
                | PeExportSelection::NameNotFound
                | PeExportSelection::OrdinalBeforeBase { .. }
                | PeExportSelection::OrdinalOutOfRange { .. },
            )
            | Err(_) => 0,
        };
        selection_rows =
            charge_selection_rows(index, selection_rows, rows, limits.max_selection_rows)?;
        selections.push(result);
    }
    Ok(PeExportBatch {
        selection_rows,
        selections,
    })
}

fn charge_selection_rows(
    index: usize,
    total: u64,
    rows: u64,
    limit: u64,
) -> Result<u64, PeExportBatchError> {
    let total = total
        .checked_add(rows)
        .ok_or(PeExportBatchError::SelectionRowsOverflow { index, total, rows })?;
    if total > limit {
        return Err(PeExportBatchError::SelectionRowsExceeded {
            index,
            total,
            limit,
        });
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::{PeExportBatchError, charge_selection_rows};
    #[test]
    fn synthetic_lengths_accept_exact_limits_and_u64_maximum() {
        for (total, rows, limit, expected) in [
            (0, 0, 0, Ok(0)),
            (u64::MAX, 0, u64::MAX, Ok(u64::MAX)),
            (u64::MAX - 1, 1, u64::MAX, Ok(u64::MAX)),
            (0, u64::MAX, u64::MAX, Ok(u64::MAX)),
            (0, 0, u64::MAX, Ok(0)),
        ] {
            assert_eq!(charge_selection_rows(7, total, rows, limit), expected);
        }
    }
    #[test]
    fn synthetic_lengths_preserve_overflow_before_limit_errors() {
        for (total, rows, limit, expected) in [
            (
                0,
                1,
                0,
                Err(PeExportBatchError::SelectionRowsExceeded {
                    index: 7,
                    total: 1,
                    limit: 0,
                }),
            ),
            (
                u64::MAX,
                1,
                u64::MAX,
                Err(PeExportBatchError::SelectionRowsOverflow {
                    index: 7,
                    total: u64::MAX,
                    rows: 1,
                }),
            ),
            (
                u64::MAX - 1,
                2,
                u64::MAX,
                Err(PeExportBatchError::SelectionRowsOverflow {
                    index: 7,
                    total: u64::MAX - 1,
                    rows: 2,
                }),
            ),
            (
                0,
                u64::MAX,
                u64::MAX - 1,
                Err(PeExportBatchError::SelectionRowsExceeded {
                    index: 7,
                    total: u64::MAX,
                    limit: u64::MAX - 1,
                }),
            ),
            (
                1,
                u64::MAX,
                u64::MAX,
                Err(PeExportBatchError::SelectionRowsOverflow {
                    index: 7,
                    total: 1,
                    rows: u64::MAX,
                }),
            ),
        ] {
            assert_eq!(charge_selection_rows(7, total, rows, limit), expected);
        }
    }
}
