use super::batch::{charge_selection_rows, check_query_count};
use super::evidence_lookup::EvidenceLookup;
use super::{
    PeExportBatchError, PeExportBatchLimits, PeExportEvidence, PeExportEvidenceLookupError,
    PeExportEvidenceLookupLimits, PeExportQuery, PeExportSelection,
};

/// ordered query results borrowing only the retained evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeExportEvidenceBatch<'e> {
    pub selection_rows: u64,
    pub selections: Vec<Result<PeExportSelection<'e>, PeExportEvidenceLookupError>>,
}

/// bounds an ordered batch over retained export evidence after image release.
/// query count is checked before lookup or allocation; empty batches do not
/// inspect evidence. each required view is admitted lazily once per batch under
/// fixed per-query limits. errors remain ordered results and cost zero
/// selection rows, as do absent, missing and out-of-range outcomes. selected
/// targets cost one row, including empty slots; ambiguity costs every match.
/// cumulative rows are checked before append. each call starts fresh budgets.
///
/// results may outlive query strings and the list, but not evidence. limits bound
/// logical rows, queries and per-query table text, not allocator capacity, process
/// memory, elapsed time or cancellation. a refused query may already have
/// allocated its result. name and ordinal views retain independent admission
/// results, including errors and absence, only for this call. required-view
/// errors and structural checks retain their precedence; no provider selection
/// or provenance check is added.
///
/// # errors
/// query-count refusal precedes all lookup. cumulative selection-row overflow
/// precedes its limit refusal, with the offending query index and attempted
/// operands. an outer refusal returns no partial batch.
///
/// ```
/// use ring3_core::{PeExportEvidence, PeExportEvidenceLookupLimits,
///     PeExportBatchLimits, PeExportQuery, PeExportSelection,
///     lookup_pe_export_evidence_batch};
/// let evidence = PeExportEvidence {
///     total_rows: 0, total_text_bytes: 0,
///     directory: Ok(None), addresses: Ok(None), names: Ok(None),
/// };
/// let batch = {
///     let name = String::from("entry");
///     lookup_pe_export_evidence_batch(&evidence, &[PeExportQuery::Name(&name)],
///         PeExportEvidenceLookupLimits { max_table_rows: 0, max_table_text_bytes: 0 },
///         PeExportBatchLimits { max_queries: 1, max_selection_rows: 0 }).unwrap()
/// };
/// assert_eq!(batch.selections, vec![Ok(PeExportSelection::DirectoryAbsent)]);
/// ```
///
/// ```compile_fail,E0515
/// use ring3_core::{PeExportEvidence, PeExportEvidenceBatch,
///     PeExportEvidenceLookupLimits, PeExportBatchLimits, lookup_pe_export_evidence_batch};
/// fn escape() -> PeExportEvidenceBatch<'static> {
///     let evidence = PeExportEvidence {
///         total_rows: 0, total_text_bytes: 0,
///         directory: Ok(None), addresses: Ok(None), names: Ok(None),
///     };
///     lookup_pe_export_evidence_batch(&evidence, &[],
///         PeExportEvidenceLookupLimits { max_table_rows: 0, max_table_text_bytes: 0 },
///         PeExportBatchLimits { max_queries: 0, max_selection_rows: 0 }).unwrap()
/// }
/// ```
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn lookup_pe_export_evidence_batch<'e>(
    evidence: &'e PeExportEvidence,
    queries: &[PeExportQuery<'_>],
    query_limits: PeExportEvidenceLookupLimits,
    batch_limits: PeExportBatchLimits,
) -> Result<PeExportEvidenceBatch<'e>, PeExportBatchError> {
    check_query_count(queries.len(), batch_limits.max_queries)?;
    let mut lookup = EvidenceLookup::new(evidence, query_limits);
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
            charge_selection_rows(index, selection_rows, rows, batch_limits.max_selection_rows)?;
        selections.push(result);
    }
    Ok(PeExportEvidenceBatch {
        selection_rows,
        selections,
    })
}
