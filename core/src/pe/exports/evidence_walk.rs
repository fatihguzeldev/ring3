use super::evidence_batch::lookup_with;
use super::evidence_lookup::EvidenceLookup;
use super::walk::walk_with;
use super::{
    PeExportBatchLimits, PeExportEvidence, PeExportEvidenceLookupError,
    PeExportEvidenceLookupLimits, PeExportQuery, PeForwarderRoute, PeForwarderWalk,
    PeForwarderWalkError, PeForwarderWalkLimits,
};
use std::collections::BTreeMap;

/// traversal refusals retain their existing operands; provider errors use the owned view contract.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PeForwarderEvidenceWalkError<'e> {
    Walk(PeForwarderWalkError<'e>),
    Provider {
        hop: u64,
        source_index: u32,
        cause: PeExportEvidenceLookupError,
    },
}

impl<'e> From<PeForwarderWalkError<'e>> for PeForwarderEvidenceWalkError<'e> {
    fn from(error: PeForwarderWalkError<'e>) -> Self {
        Self::Walk(error)
    }
}

/// walks retained export evidence using caller-supplied source-context routes.
/// source positions, exact route tokens, cycle identity and aggregate walk limits
/// follow `walk_pe_export_forwarders`. required views are admitted lazily once
/// per source position under fixed query limits within this call. name and
/// ordinal views remain independent; this does not authenticate evidence.
/// query limits count complete required table rows/text, independently of the
/// walk's cumulative selection rows and route/root/forwarder text. unused evidence
/// views and sources are not inspected. results and missing-route errors borrow
/// only evidence, and may outlive the source list, routes and root query text.
///
/// # errors
/// source/route/root admission precedes any lookup. traversal checks hops,
/// lookup/row admission, cycle, decoding, then exact routing. common refusals are
/// `Walk`; every owned query error, including stored reader errors, is `Provider`.
/// a refusal returns no partial walk. limits do not cap temporary allocations,
/// process memory or elapsed time, and do not provide cancellation or identity.
///
/// # panics
/// only if an internal decoder or single-query batch invariant is violated.
///
/// ```
/// use ring3_core::{PeExportEvidence, PeExportEvidenceLookupLimits, PeExportQuery,
///     PeExportSelection, PeForwarderWalkLimits, walk_pe_export_evidence_forwarders};
/// let evidence = PeExportEvidence { total_rows: 0, total_text_bytes: 0,
///     directory: Ok(None), addresses: Ok(None), names: Ok(None) };
/// let result = walk_pe_export_evidence_forwarders(&[&evidence], &[], 0,
///     PeExportQuery::Ordinal(1), PeExportEvidenceLookupLimits {
///         max_table_rows: 0, max_table_text_bytes: 0 }, PeForwarderWalkLimits {
///         max_sources: 1, max_routes: 0, max_hops: 1,
///         max_selection_rows: 0, max_text_bytes: 0 }).unwrap();
/// assert_eq!(result.steps[0].selection, PeExportSelection::DirectoryAbsent);
/// ```
///
/// ```compile_fail,E0515
/// use ring3_core::{PeExportEvidence, PeExportEvidenceLookupLimits, PeExportQuery,
///     PeForwarderWalk, PeForwarderWalkLimits, walk_pe_export_evidence_forwarders};
/// fn escape() -> PeForwarderWalk<'static> {
///     let evidence = PeExportEvidence { total_rows: 0, total_text_bytes: 0,
///         directory: Ok(None), addresses: Ok(None), names: Ok(None) };
///     walk_pe_export_evidence_forwarders(&[&evidence], &[], 0,
///         PeExportQuery::Ordinal(1), PeExportEvidenceLookupLimits {
///             max_table_rows: 0, max_table_text_bytes: 0 }, PeForwarderWalkLimits {
///             max_sources: 1, max_routes: 0, max_hops: 1,
///             max_selection_rows: 0, max_text_bytes: 0 }).unwrap()
/// }
/// ```
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn walk_pe_export_evidence_forwarders<'e>(
    sources: &[&'e PeExportEvidence],
    routes: &[PeForwarderRoute<'_>],
    root_source_index: u32,
    root_query: PeExportQuery<'_>,
    query_limits: PeExportEvidenceLookupLimits,
    limits: PeForwarderWalkLimits,
) -> Result<PeForwarderWalk<'e>, PeForwarderEvidenceWalkError<'e>> {
    EvidenceWalk::new(sources, query_limits).walk(routes, root_source_index, root_query, limits)
}

pub(super) struct EvidenceWalk<'sources, 'e> {
    sources: &'sources [&'e PeExportEvidence],
    query_limits: PeExportEvidenceLookupLimits,
    lookups: BTreeMap<u32, EvidenceLookup<'e>>,
}

impl<'sources, 'e> EvidenceWalk<'sources, 'e> {
    pub(super) fn new(
        sources: &'sources [&'e PeExportEvidence],
        query_limits: PeExportEvidenceLookupLimits,
    ) -> Self {
        Self {
            sources,
            query_limits,
            lookups: BTreeMap::new(),
        }
    }

    pub(super) fn walk(
        &mut self,
        routes: &[PeForwarderRoute<'_>],
        root_source_index: u32,
        root_query: PeExportQuery<'_>,
        limits: PeForwarderWalkLimits,
    ) -> Result<PeForwarderWalk<'e>, PeForwarderEvidenceWalkError<'e>> {
        walk_with(
            self.sources.len(),
            routes,
            root_source_index,
            root_query,
            limits,
            || {
                move |source_index, query, hop, used| {
                    let lookup = self.lookups.entry(source_index).or_insert_with(|| {
                        EvidenceLookup::new(self.sources[source_index as usize], self.query_limits)
                    });
                    let mut batch = lookup_with(
                        lookup,
                        std::iter::once(query),
                        PeExportBatchLimits {
                            max_queries: 1,
                            max_selection_rows: limits.max_selection_rows - used,
                        },
                    )
                    .map_err(|cause| PeForwarderWalkError::SelectionRows {
                        hop,
                        source_index,
                        used,
                        cause,
                    })?;
                    let total = used.checked_add(batch.selection_rows).ok_or(
                        PeForwarderWalkError::SelectionRowsOverflow {
                            hop,
                            source_index,
                            total: used,
                            rows: batch.selection_rows,
                        },
                    )?;
                    let selection = batch
                        .selections
                        .pop()
                        .expect("one admitted query has one result")
                        .map_err(|cause| PeForwarderEvidenceWalkError::Provider {
                            hop,
                            source_index,
                            cause,
                        })?;
                    Ok((selection, total))
                }
            },
        )
    }
}
