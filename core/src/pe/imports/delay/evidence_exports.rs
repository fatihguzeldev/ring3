use super::super::evidence_exports::lookup_entry_exports;
use super::{PeDelayImportEvidence, PeDelayImportExportError, PeOwnedDelayImportLookup};
use crate::pe::exports::{
    PeExportBatchLimits, PeExportEvidence, PeExportEvidenceBatch, PeExportEvidenceLookupLimits,
};

/// a retained delay-import record and aligned, independently borrowed exports.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeDelayImportEvidenceExportBatch<'importer, 'provider> {
    pub imports: &'importer PeOwnedDelayImportLookup,
    pub exports: PeExportEvidenceBatch<'provider>,
}

/// matches one retained delay-import record against an explicit owned provider.
/// only `importer.lookups` is used. absent and present-empty tables have no
/// selectable descriptors. unrelated raw/name views and reported totals are
/// ignored; no unselected record is inspected. the selected record is borrowed.
///
/// actual selected entry count is admitted before symbol traversal. queries are
/// borrowed lazily without an intermediate list. exact names and widened u16
/// ordinals use the same private mapping as static imports and the bounded
/// owned export batch.
/// provider errors remain aligned with entries; empty entries skip provider
/// inspection. results retain two independent evidence lifetimes without image
/// bytes, import-record clones or text copies.
///
/// caller-built delay kind, attributes, coordinates, raw values, terminator and
/// dll text remain opaque metadata; symbols drive queries without pe revalidation.
/// no provider identity/discovery, delay loading, iat fallback/binding or execution
/// is added. limits are logical query/table/selection caps, not allocator, elapsed
/// time or cancellation guarantees.
///
/// # errors
/// stored delay lookup errors precede descriptor selection, then query count and
/// provider batch limits. outer failure returns no partial association; provider
/// failures remain ordered per-query results.
///
/// ```
/// use ring3_core::{PeDelayImportEvidence, PeExportEvidence,
///     PeExportEvidenceLookupLimits, PeExportBatchLimits, PeDelayImportExportError,
///     lookup_pe_delay_import_evidence_exports};
/// let importer = PeDelayImportEvidence {
///     total_rows: 0, total_text_bytes: 0, descriptors: Ok(None), names: Ok(None), lookups: Ok(None),
/// };
/// let provider = PeExportEvidence {
///     total_rows: 0, total_text_bytes: 0,
///     directory: Ok(None), addresses: Ok(None), names: Ok(None),
/// };
/// assert_eq!(lookup_pe_delay_import_evidence_exports(&importer, 0, &provider,
///     PeExportEvidenceLookupLimits { max_table_rows: 0, max_table_text_bytes: 0 },
///     PeExportBatchLimits { max_queries: 0, max_selection_rows: 0 }),
///     Err(PeDelayImportExportError::DescriptorNotFound { descriptor_index: 0, descriptor_count: 0 }));
/// ```
///
/// ```compile_fail,E0515
/// use ring3_core::{PeDelayImportEvidence, PeExportEvidence, PeDelayImportEvidenceExportBatch,
///     PeExportEvidenceLookupLimits, PeExportBatchLimits, lookup_pe_delay_import_evidence_exports};
/// fn escape(provider: &PeExportEvidence) -> PeDelayImportEvidenceExportBatch<'static, '_> {
///     let importer = PeDelayImportEvidence {
///         total_rows: 0, total_text_bytes: 0, descriptors: Ok(None), names: Ok(None), lookups: Ok(None),
///     };
///     lookup_pe_delay_import_evidence_exports(&importer, 0, provider,
///         PeExportEvidenceLookupLimits { max_table_rows: 0, max_table_text_bytes: 0 },
///         PeExportBatchLimits { max_queries: 0, max_selection_rows: 0 }).unwrap()
/// }
/// ```
///
/// ```compile_fail,E0515
/// use ring3_core::{PeDelayImportEvidence, PeExportEvidence, PeDelayImportEvidenceExportBatch,
///     PeExportEvidenceLookupLimits, PeExportBatchLimits, lookup_pe_delay_import_evidence_exports};
/// fn escape(importer: &PeDelayImportEvidence) -> PeDelayImportEvidenceExportBatch<'_, 'static> {
///     let provider = PeExportEvidence {
///         total_rows: 0, total_text_bytes: 0,
///         directory: Ok(None), addresses: Ok(None), names: Ok(None),
///     };
///     lookup_pe_delay_import_evidence_exports(importer, 0, &provider,
///         PeExportEvidenceLookupLimits { max_table_rows: 0, max_table_text_bytes: 0 },
///         PeExportBatchLimits { max_queries: 0, max_selection_rows: 0 }).unwrap()
/// }
/// ```
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn lookup_pe_delay_import_evidence_exports<'importer, 'provider>(
    importer: &'importer PeDelayImportEvidence,
    descriptor_index: u16,
    provider: &'provider PeExportEvidence,
    query_limits: PeExportEvidenceLookupLimits,
    batch_limits: PeExportBatchLimits,
) -> Result<PeDelayImportEvidenceExportBatch<'importer, 'provider>, PeDelayImportExportError> {
    let table = importer
        .lookups
        .as_ref()
        .map_err(|e| PeDelayImportExportError::Imports(*e))?;
    let descriptors = table.as_ref().map_or(&[][..], |t| t.imports.as_slice());
    let imports = descriptors.get(usize::from(descriptor_index)).ok_or(
        PeDelayImportExportError::DescriptorNotFound {
            descriptor_index,
            descriptor_count: descriptors.len(),
        },
    )?;
    let exports = lookup_entry_exports(&imports.entries, provider, query_limits, batch_limits)
        .map_err(PeDelayImportExportError::ExportBatch)?;
    Ok(PeDelayImportEvidenceExportBatch { imports, exports })
}
