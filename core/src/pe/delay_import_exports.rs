use super::{
    PeDelayImportLookup, PeDelayImportLookupError, PeExportBatch, PeExportBatchError,
    PeExportBatchLimits, PeExportQuery, PeImportSymbol, lookup_pe_export_batch,
    parse_pe_delay_import_lookups,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeDelayImportExportError {
    Imports(PeDelayImportLookupError),
    DescriptorNotFound {
        descriptor_index: u16,
        descriptor_count: usize,
    },
    ExportBatch(PeExportBatchError),
}

/// delay import entries and their positionally aligned export results from an explicit provider.
/// text retains the lifetime of its own input image, independently of the other image.
///
/// ```compile_fail
/// use ring3_core::{PeDelayImportExportBatch, PeExportBatchLimits, lookup_pe_delay_import_exports};
/// fn escape(provider: &[u8]) -> PeDelayImportExportBatch<'static, '_> {
///     let importer = vec![0; 64];
///     lookup_pe_delay_import_exports(&importer, 0, provider,
///         PeExportBatchLimits { max_queries: 1, max_selection_rows: 1 }).unwrap()
/// }
/// ```
///
/// ```compile_fail
/// use ring3_core::{PeDelayImportExportBatch, PeExportBatchLimits, lookup_pe_delay_import_exports};
/// fn escape(importer: &[u8]) -> PeDelayImportExportBatch<'_, 'static> {
///     let provider = vec![0; 64];
///     lookup_pe_delay_import_exports(importer, 0, &provider,
///         PeExportBatchLimits { max_queries: 1, max_selection_rows: 1 }).unwrap()
/// }
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeDelayImportExportBatch<'importer, 'provider> {
    pub imports: PeDelayImportLookup<'importer>,
    pub exports: PeExportBatch<'provider>,
}

/// matches one delay import descriptor's symbols against caller-supplied provider bytes.
///
/// the complete delay table, dll names and lookups are parsed before selecting the zero-based descriptor index.
/// names are exact and case-sensitive; hints remain metadata. import ordinals are
/// widened without truncation. one export batch preserves entry order, including
/// per-query provider errors. an empty selected descriptor does not parse the provider.
/// absent and empty tables have no selectable descriptors. zero int addresses never
/// fall back to iat bytes, and unsupported delay attributes keep their raw reader error.
///
/// limits apply to derived provider queries and selection rows after import parsing.
/// they do not cap input bytes, temporary import/query allocations or process memory.
/// raw targets are returned without architecture checks, module discovery, forwarder
/// traversal, address binding or execution.
///
/// # errors
/// complete importer errors precede descriptor selection, which precedes export
/// batch limits. outer failure returns no partial batch. provider parse errors
/// remain ordered per-query results inside a successful batch.
///
/// # example
/// ```
/// use ring3_core::{PeDelayImportExportError, PeExportBatchLimits, lookup_pe_delay_import_exports};
/// let result = lookup_pe_delay_import_exports(b"", 0, b"",
///     PeExportBatchLimits { max_queries: 0, max_selection_rows: 0 });
/// assert!(matches!(result, Err(PeDelayImportExportError::Imports(_))));
/// ```
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn lookup_pe_delay_import_exports<'importer, 'provider>(
    importer_bytes: &'importer [u8],
    descriptor_index: u16,
    provider_bytes: &'provider [u8],
    limits: PeExportBatchLimits,
) -> Result<PeDelayImportExportBatch<'importer, 'provider>, PeDelayImportExportError> {
    let descriptors = parse_pe_delay_import_lookups(importer_bytes)
        .map_err(PeDelayImportExportError::Imports)?
        .map_or_else(Vec::new, |table| table.imports);
    let descriptor_count = descriptors.len();
    let imports = descriptors
        .into_iter()
        .nth(usize::from(descriptor_index))
        .ok_or(PeDelayImportExportError::DescriptorNotFound {
            descriptor_index,
            descriptor_count,
        })?;
    let queries: Vec<_> = imports
        .entries
        .iter()
        .map(|entry| match entry.symbol {
            PeImportSymbol::ByName { name, .. } => PeExportQuery::Name(name),
            PeImportSymbol::Ordinal(ordinal) => PeExportQuery::Ordinal(u32::from(ordinal)),
        })
        .collect();
    let exports = lookup_pe_export_batch(provider_bytes, &queries, limits)
        .map_err(PeDelayImportExportError::ExportBatch)?;
    Ok(PeDelayImportExportBatch { imports, exports })
}
