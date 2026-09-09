use super::optional::{read_u16, read_u32};
use super::{PeCertificateError, PeCertificateTable, parse_pe_certificate_table};
use crate::FileOffset;

const ENTRY_LIMIT: u16 = 256;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeCertificateEntry<'a> {
    pub file_offset: FileOffset,
    pub length: u32,
    pub revision: u16,
    pub certificate_type: u16,
    /// declared bytes after the header, including any padding inside length.
    pub raw_body: &'a [u8],
    /// bytes between declared length and the next eight-byte boundary; unvalidated.
    pub alignment_padding: &'a [u8],
}

/// ordered raw records, not validated certificates or signatures.
///
/// ```compile_fail
/// use ring3_core::{PeCertificateEntryTable, parse_pe_certificate_entries};
///
/// fn escape() -> PeCertificateEntryTable<'static> {
///     let bytes = vec![0; 512];
///     parse_pe_certificate_entries(&bytes).unwrap().unwrap()
/// }
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeCertificateEntryTable<'a> {
    pub table: PeCertificateTable<'a>,
    pub entries: Vec<PeCertificateEntry<'a>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeCertificateEntryError {
    Table(PeCertificateError),
    EntryLimitExceeded {
        entry_index: u16,
        limit: u16,
    },
    InvalidEntryLength {
        entry_index: u16,
        length: u32,
    },
    EntryExceedsTable {
        entry_index: u16,
        length: u32,
        padded_length: u64,
        remaining: u32,
    },
}

/// reads at most 256 raw certificate headers without scanning or copying bodies.
/// unknown scalars, duplicates and nonzero padding are preserved without validation.
/// exact table size terminates traversal; a zero length is not a sentinel.
///
/// # errors
/// validates the raw table first, then each entry's count budget, minimum length,
/// and rounded extent. failures never return a partial table.
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn parse_pe_certificate_entries(
    bytes: &[u8],
) -> Result<Option<PeCertificateEntryTable<'_>>, PeCertificateEntryError> {
    let Some(table) = parse_pe_certificate_table(bytes).map_err(PeCertificateEntryError::Table)?
    else {
        return Ok(None);
    };
    let mut entries = Vec::new();
    let mut consumed = 0_u32;
    for entry_index in 0..=ENTRY_LIMIT {
        if consumed == table.size {
            return Ok(Some(PeCertificateEntryTable { table, entries }));
        }
        if entry_index == ENTRY_LIMIT {
            return Err(PeCertificateEntryError::EntryLimitExceeded {
                entry_index,
                limit: ENTRY_LIMIT,
            });
        }
        // table size and every consumed step are aligned, so this header fits.
        let record = &table.raw_bytes[consumed as usize..];
        let length = read_u32(record, 0);
        if length < 8 {
            return Err(PeCertificateEntryError::InvalidEntryLength {
                entry_index,
                length,
            });
        }
        let padded_length = (u64::from(length) + 7) & !7;
        let remaining = table.size - consumed;
        let step = u32::try_from(padded_length)
            .ok()
            .filter(|&step| step <= remaining)
            .ok_or(PeCertificateEntryError::EntryExceedsTable {
                entry_index,
                length,
                padded_length,
                remaining,
            })?;
        let body_end = (consumed + length) as usize;
        let end = consumed + step;
        entries.push(PeCertificateEntry {
            file_offset: FileOffset::new(table.file_offset.get() + u64::from(consumed)),
            length,
            revision: read_u16(record, 4),
            certificate_type: read_u16(record, 6),
            raw_body: &table.raw_bytes[(consumed + 8) as usize..body_end],
            alignment_padding: &table.raw_bytes[body_end..end as usize],
        });
        consumed = end;
    }
    unreachable!("the final iteration returns completion or the entry limit error")
}
