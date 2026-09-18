use super::evidence_walk::EvidenceWalk;
use super::{
    PeExportEvidence, PeExportEvidenceLookupLimits, PeExportQuery, PeForwarderEvidenceWalkError,
    PeForwarderRoute, PeForwarderWalk, PeForwarderWalkLimits,
};

/// query count and aggregate metrics of complete successful walks only.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeForwarderWalkBatchLimits {
    pub max_queries: u64,
    pub max_successful_steps: u64,
    pub max_successful_selection_rows: u64,
    pub max_successful_text_bytes: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeForwarderWalkBatchMetric {
    SuccessfulSteps,
    SuccessfulSelectionRows,
    SuccessfulTextBytes,
}

/// `used` is the pre-add total; `amount` belongs to the offending successful walk.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeForwarderWalkBatchError {
    QueryCountExceeded {
        count: u64,
        limit: u64,
    },
    OutputOverflow {
        metric: PeForwarderWalkBatchMetric,
        index: usize,
        used: u64,
        amount: u64,
    },
    OutputLimitExceeded {
        metric: PeForwarderWalkBatchMetric,
        index: usize,
        used: u64,
        amount: u64,
        limit: u64,
    },
}

/// ordered complete results; only successful walks contribute to these totals.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeForwarderEvidenceWalkBatch<'e> {
    pub successful_steps: u64,
    pub successful_selection_rows: u64,
    pub successful_text_bytes: u64,
    pub walks: Vec<Result<PeForwarderWalk<'e>, PeForwarderEvidenceWalkError<'e>>>,
}

fn charge(
    metric: PeForwarderWalkBatchMetric,
    index: usize,
    used: u64,
    amount: u64,
    limit: u64,
) -> Result<u64, PeForwarderWalkBatchError> {
    let total = used
        .checked_add(amount)
        .ok_or(PeForwarderWalkBatchError::OutputOverflow {
            metric,
            index,
            used,
            amount,
        })?;
    if total > limit {
        return Err(PeForwarderWalkBatchError::OutputLimitExceeded {
            metric,
            index,
            used,
            amount,
            limit,
        });
    }
    Ok(total)
}

/// admits aggregate successful results over ordered owned-evidence walks.
/// query count precedes result allocation and all single-walk calls. empty
/// queries do not inspect sources, routes or root indices. each query starts
/// fresh walk budgets under uniform view/walk limits. required export views are
/// admitted lazily once per source position for this batch; name and ordinal
/// views remain independent. common source/route/root admission still repeats,
/// and every provider or walk error stays in order.
///
/// only complete successes charge steps, selection rows, then walk text bytes,
/// before append. text includes repeated route/root admission for each successful
/// walk. failures cost zero in these totals even after prior traversal work.
/// error payload text, temporary allocations, capacity, process memory, elapsed
/// time and cancellation are not bounded by these aggregate totals.
///
/// results borrow only export evidence; source lists, routes and query text may
/// be dropped. view admission results are retained only within this call;
/// selections and walks are not cached. no provider discovery, identity checks
/// or binding occur.
///
/// # errors
/// count refusal precedes traversal. each aggregate addition checks overflow
/// before its limit; steps precede rows, which precede text. the error identifies
/// the query and pre-add operands. outer refusal returns no partial batch, though
/// the offending complete walk may already have allocated its result.
///
/// ```
/// use ring3_core::{PeExportEvidenceLookupLimits, PeForwarderWalkLimits,
///     PeForwarderWalkBatchLimits, walk_pe_export_evidence_forwarders_batch};
/// let result = walk_pe_export_evidence_forwarders_batch(&[], &[], u32::MAX, &[],
///     PeExportEvidenceLookupLimits { max_table_rows: 0, max_table_text_bytes: 0 },
///     PeForwarderWalkLimits { max_sources: 0, max_routes: 0, max_hops: 0,
///         max_selection_rows: 0, max_text_bytes: 0 },
///     PeForwarderWalkBatchLimits { max_queries: 0, max_successful_steps: 0,
///         max_successful_selection_rows: 0, max_successful_text_bytes: 0 }).unwrap();
/// assert!(result.walks.is_empty());
/// ```
///
/// ```compile_fail,E0515
/// use ring3_core::{PeExportEvidence, PeExportEvidenceLookupLimits,
///     PeForwarderEvidenceWalkBatch, PeForwarderWalkLimits, PeForwarderWalkBatchLimits,
///     walk_pe_export_evidence_forwarders_batch};
/// fn escape() -> PeForwarderEvidenceWalkBatch<'static> {
///     let evidence = PeExportEvidence { total_rows: 0, total_text_bytes: 0,
///         directory: Ok(None), addresses: Ok(None), names: Ok(None) };
///     walk_pe_export_evidence_forwarders_batch(&[&evidence], &[], 0, &[],
///         PeExportEvidenceLookupLimits { max_table_rows: 0, max_table_text_bytes: 0 },
///         PeForwarderWalkLimits { max_sources: 1, max_routes: 0, max_hops: 1,
///             max_selection_rows: 0, max_text_bytes: 0 },
///         PeForwarderWalkBatchLimits { max_queries: 0, max_successful_steps: 0,
///             max_successful_selection_rows: 0, max_successful_text_bytes: 0 }).unwrap()
/// }
/// ```
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn walk_pe_export_evidence_forwarders_batch<'e>(
    sources: &[&'e PeExportEvidence],
    routes: &[PeForwarderRoute<'_>],
    root_source_index: u32,
    queries: &[PeExportQuery<'_>],
    query_limits: PeExportEvidenceLookupLimits,
    walk_limits: PeForwarderWalkLimits,
    batch_limits: PeForwarderWalkBatchLimits,
) -> Result<PeForwarderEvidenceWalkBatch<'e>, PeForwarderWalkBatchError> {
    const {
        assert!(usize::BITS <= 64);
    }
    let count = queries.len() as u64;
    if count > batch_limits.max_queries {
        return Err(PeForwarderWalkBatchError::QueryCountExceeded {
            count,
            limit: batch_limits.max_queries,
        });
    }
    let mut batch = PeForwarderEvidenceWalkBatch {
        successful_steps: 0,
        successful_selection_rows: 0,
        successful_text_bytes: 0,
        walks: Vec::new(),
    };
    let mut owner = EvidenceWalk::new(sources, query_limits);
    for (index, &query) in queries.iter().enumerate() {
        let result = owner.walk(routes, root_source_index, query, walk_limits);
        if let Ok(walk) = &result {
            batch.successful_steps = charge(
                PeForwarderWalkBatchMetric::SuccessfulSteps,
                index,
                batch.successful_steps,
                walk.steps.len() as u64,
                batch_limits.max_successful_steps,
            )?;
            batch.successful_selection_rows = charge(
                PeForwarderWalkBatchMetric::SuccessfulSelectionRows,
                index,
                batch.successful_selection_rows,
                walk.selection_rows,
                batch_limits.max_successful_selection_rows,
            )?;
            batch.successful_text_bytes = charge(
                PeForwarderWalkBatchMetric::SuccessfulTextBytes,
                index,
                batch.successful_text_bytes,
                walk.text_bytes,
                batch_limits.max_successful_text_bytes,
            )?;
        }
        batch.walks.push(result);
    }
    Ok(batch)
}

#[cfg(test)]
mod tests {
    use super::{PeForwarderWalkBatchError as Error, PeForwarderWalkBatchMetric as Metric, charge};

    #[test]
    fn each_metric_checks_exact_overflow_and_limit_operands() {
        for metric in [
            Metric::SuccessfulSteps,
            Metric::SuccessfulSelectionRows,
            Metric::SuccessfulTextBytes,
        ] {
            assert_eq!(charge(metric, 9, u64::MAX - 1, 1, u64::MAX), Ok(u64::MAX));
            for limit in [0, u64::MAX] {
                assert_eq!(
                    charge(metric, 9, u64::MAX, 1, limit),
                    Err(Error::OutputOverflow {
                        metric,
                        index: 9,
                        used: u64::MAX,
                        amount: 1,
                    })
                );
            }
            assert_eq!(
                charge(metric, 9, 0, 1, 0),
                Err(Error::OutputLimitExceeded {
                    metric,
                    index: 9,
                    used: 0,
                    amount: 1,
                    limit: 0,
                })
            );
        }
    }
}
