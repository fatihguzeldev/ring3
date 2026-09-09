use super::{
    PeDebugDirectoryError, PeDebugDirectoryTable, PeHeaderError, Reader, parse_pe_debug_directory,
};
use crate::FileOffset;

/// physical file bytes without an inferred mapped header or section source.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeDebugPayloadRange<'a> {
    pub file_offset: FileOffset,
    pub raw_bytes: &'a [u8],
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeDebugPayload<'a> {
    pub entry_index: u16,
    /// no physical coordinate is assigned to a zero-size payload.
    pub range: Option<PeDebugPayloadRange<'a>>,
}

/// one borrowed file view per raw debug entry, preserving aliases and overlaps.
///
/// ```compile_fail
/// use ring3_core::{PeDebugPayloadTable, parse_pe_debug_payloads};
///
/// fn escape() -> Option<PeDebugPayloadTable<'static>> {
///     let bytes = vec![0; 64];
///     parse_pe_debug_payloads(&bytes).unwrap()
/// }
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeDebugPayloadTable<'a> {
    pub directory_table: PeDebugDirectoryTable,
    pub payloads: Vec<PeDebugPayload<'a>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeDebugPayloadError {
    Directory(PeDebugDirectoryError),
    PayloadRange {
        entry_index: u16,
        file_offset: FileOffset,
        size: u32,
        cause: PeHeaderError,
    },
}

/// borrows opaque payload bytes using raw file pointers, independently of rvas.
/// zero-size records keep their raw coordinates in the directory table and have
/// no range. nonempty ranges may occupy headers, sections, overlay or boundaries
/// between them. types and coordinate consistency are not interpreted.
/// the directory's 256-entry limit bounds view metadata; payloads are not copied,
/// scanned or decoded, and no additional payload byte budget is imposed.
///
/// # errors
/// validates the complete debug directory before reading any payload. then reads
/// nonempty physical ranges in entry order with widened file arithmetic, retaining
/// the first failing entry index, coordinates and complete file-reader error.
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn parse_pe_debug_payloads(
    bytes: &[u8],
) -> Result<Option<PeDebugPayloadTable<'_>>, PeDebugPayloadError> {
    let Some(directory_table) =
        parse_pe_debug_directory(bytes).map_err(PeDebugPayloadError::Directory)?
    else {
        return Ok(None);
    };
    let reader = Reader { bytes };
    let mut payloads = Vec::new();
    for (entry_index, entry) in (0_u16..).zip(&directory_table.entries) {
        let range = if entry.size_of_data == 0 {
            None
        } else {
            let (raw_bytes, _) = reader
                .read(entry.pointer_to_raw_data, u64::from(entry.size_of_data))
                .map_err(|cause| PeDebugPayloadError::PayloadRange {
                    entry_index,
                    file_offset: entry.pointer_to_raw_data,
                    size: entry.size_of_data,
                    cause,
                })?;
            Some(PeDebugPayloadRange {
                file_offset: entry.pointer_to_raw_data,
                raw_bytes,
            })
        };
        payloads.push(PeDebugPayload { entry_index, range });
    }
    Ok(Some(PeDebugPayloadTable {
        directory_table,
        payloads,
    }))
}
