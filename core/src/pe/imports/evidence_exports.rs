use super::{
    PeImportExportError, PeOwnedImportLookup, PeOwnedImportSymbol, PeStaticImportEvidence,
};
use crate::pe::exports::{
    PeExportBatchError, PeExportBatchLimits, PeExportEvidence, PeExportEvidenceBatch,
    PeExportEvidenceLookupLimits, PeExportQuery, lookup_pe_export_evidence_batch,
};

/// a retained import record and aligned selections borrowing their own evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeImportEvidenceExportBatch<'importer, 'provider> {
    pub imports: &'importer PeOwnedImportLookup,
    pub exports: PeExportEvidenceBatch<'provider>,
}

/// matches one retained static-import descriptor against an explicit provider.
/// only `importer.lookups` is read; unrelated descriptor views and reported totals
/// are ignored. the selected record is borrowed without cloning. its actual entry
/// count is admitted before query allocation or traversal of any symbol.
///
/// each entry supplies one exact name or losslessly widened ordinal in order.
/// hints, raw values, coordinates and dll names remain opaque metadata; they do
/// not validate symbols, identify a provider or establish image provenance.
/// caller-built unicode/empty names and inconsistent raw fields stay unchanged.
/// no unselected import record is inspected and no importer text is copied.
///
/// provider queries follow `lookup_pe_export_evidence_batch`, including uniform
/// required-view admission, ordered per-query errors and aggregate row charging.
/// empty selected entries do not inspect the provider. results borrow the two
/// evidence objects independently; original image bytes are never required.
/// limits do not cap existing allocations, capacity, elapsed time or cancellation.
/// no dll discovery, architecture check, forwarder traversal or binding is added.
///
/// # errors
/// stored importer errors precede descriptor selection, which precedes query
/// count admission and provider batch limits. outer failure returns no partial
/// association; provider errors remain aligned per-query results.
///
/// ```
/// use ring3_core::{PeStaticImportEvidence, PeExportEvidence,
///     PeExportEvidenceLookupLimits, PeExportBatchLimits, PeImportExportError,
///     lookup_pe_import_evidence_exports};
/// let importer = PeStaticImportEvidence {
///     total_rows: 0, total_text_bytes: 0, descriptors: Ok(vec![]), lookups: Ok(vec![]),
/// };
/// let provider = PeExportEvidence {
///     total_rows: 0, total_text_bytes: 0,
///     directory: Ok(None), addresses: Ok(None), names: Ok(None),
/// };
/// assert_eq!(lookup_pe_import_evidence_exports(&importer, 0, &provider,
///     PeExportEvidenceLookupLimits { max_table_rows: 0, max_table_text_bytes: 0 },
///     PeExportBatchLimits { max_queries: 0, max_selection_rows: 0 }),
///     Err(PeImportExportError::DescriptorNotFound { descriptor_index: 0, descriptor_count: 0 }));
/// ```
///
/// ```compile_fail,E0515
/// use ring3_core::{PeStaticImportEvidence, PeExportEvidence, PeImportEvidenceExportBatch,
///     PeExportEvidenceLookupLimits, PeExportBatchLimits, lookup_pe_import_evidence_exports};
/// fn escape(provider: &PeExportEvidence) -> PeImportEvidenceExportBatch<'static, '_> {
///     let importer = PeStaticImportEvidence {
///         total_rows: 0, total_text_bytes: 0, descriptors: Ok(vec![]), lookups: Ok(vec![]),
///     };
///     lookup_pe_import_evidence_exports(&importer, 0, provider,
///         PeExportEvidenceLookupLimits { max_table_rows: 0, max_table_text_bytes: 0 },
///         PeExportBatchLimits { max_queries: 0, max_selection_rows: 0 }).unwrap()
/// }
/// ```
///
/// ```compile_fail,E0515
/// use ring3_core::{PeStaticImportEvidence, PeExportEvidence, PeImportEvidenceExportBatch,
///     PeExportEvidenceLookupLimits, PeExportBatchLimits, lookup_pe_import_evidence_exports};
/// fn escape(importer: &PeStaticImportEvidence) -> PeImportEvidenceExportBatch<'_, 'static> {
///     let provider = PeExportEvidence {
///         total_rows: 0, total_text_bytes: 0,
///         directory: Ok(None), addresses: Ok(None), names: Ok(None),
///     };
///     lookup_pe_import_evidence_exports(importer, 0, &provider,
///         PeExportEvidenceLookupLimits { max_table_rows: 0, max_table_text_bytes: 0 },
///         PeExportBatchLimits { max_queries: 0, max_selection_rows: 0 }).unwrap()
/// }
/// ```
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn lookup_pe_import_evidence_exports<'importer, 'provider>(
    importer: &'importer PeStaticImportEvidence,
    descriptor_index: u16,
    provider: &'provider PeExportEvidence,
    query_limits: PeExportEvidenceLookupLimits,
    batch_limits: PeExportBatchLimits,
) -> Result<PeImportEvidenceExportBatch<'importer, 'provider>, PeImportExportError> {
    let descriptors = importer
        .lookups
        .as_ref()
        .map_err(|e| PeImportExportError::Imports(*e))?;
    let imports = descriptors.get(usize::from(descriptor_index)).ok_or(
        PeImportExportError::DescriptorNotFound {
            descriptor_index,
            descriptor_count: descriptors.len(),
        },
    )?;
    let count = imports.entries.len() as u64;
    if count > batch_limits.max_queries {
        return Err(PeImportExportError::ExportBatch(
            PeExportBatchError::QueryCountExceeded {
                count,
                limit: batch_limits.max_queries,
            },
        ));
    }
    let queries: Vec<_> = imports
        .entries
        .iter()
        .map(|entry| match &entry.symbol {
            PeOwnedImportSymbol::ByName { name, .. } => PeExportQuery::Name(name),
            PeOwnedImportSymbol::Ordinal(ordinal) => PeExportQuery::Ordinal(u32::from(*ordinal)),
        })
        .collect();
    let exports = lookup_pe_export_evidence_batch(provider, &queries, query_limits, batch_limits)
        .map_err(PeImportExportError::ExportBatch)?;
    Ok(PeImportEvidenceExportBatch { imports, exports })
}
