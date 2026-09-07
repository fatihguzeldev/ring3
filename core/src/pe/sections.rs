use super::optional::{read_u16, read_u32};
use super::{PeHeaderError, PeHeaders, Reader, parse_pe_headers};
use crate::{FileOffset, RelativeVirtualAddress};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeSection<'a> {
    pub name: [u8; 8],
    pub virtual_size: u32,
    pub virtual_address: RelativeVirtualAddress,
    pub size_of_raw_data: u32,
    pub pointer_to_raw_data: FileOffset,
    pub pointer_to_relocations: FileOffset,
    pub pointer_to_line_numbers: FileOffset,
    pub number_of_relocations: u16,
    pub number_of_line_numbers: u16,
    pub characteristics: u32,
    /// physically present bytes from the same input; no virtual zero-fill is added.
    pub raw_data: &'a [u8],
}

/// ordered section metadata with a local ninety-six-record parsing limit.
///
/// ```compile_fail
/// use ring3_core::{PeSectionTable, parse_pe_sections};
///
/// fn escape() -> PeSectionTable<'static> {
///     let bytes = vec![0; 64];
///     parse_pe_sections(&bytes).unwrap()
/// }
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeSectionTable<'a> {
    pub headers: PeHeaders,
    pub sections: Vec<PeSection<'a>>,
}

/// reads section descriptors and raw bytes without selecting a virtual mapping.
///
/// # errors
/// returns header errors, a section-budget failure, table/raw bounds failures,
/// or a virtual span extending past the thirty-two-bit coordinate space.
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn parse_pe_sections(bytes: &[u8]) -> Result<PeSectionTable<'_>, PeHeaderError> {
    let headers = parse_pe_headers(bytes)?;
    let reader = Reader { bytes };
    let (_, coff_offset) = reader.read(headers.prefix.pe_offset, 4)?;
    let (_, optional_offset) = reader.read(coff_offset, 20)?;
    let (_, table_offset) = reader.read(
        optional_offset,
        u64::from(headers.prefix.size_of_optional_header),
    )?;
    let count = headers.prefix.number_of_sections;
    if count > 96 {
        let (_, count_offset) = reader.read(coff_offset, 2)?;
        return Err(PeHeaderError::SectionLimitExceeded {
            offset: count_offset,
            count,
            limit: 96,
        });
    }
    reader
        .read(table_offset, u64::from(count) * 40)
        .map_err(|error| match error {
            PeHeaderError::OutOfBounds {
                offset,
                needed,
                available,
            } => PeHeaderError::SectionTableOutOfBounds {
                offset,
                needed,
                available,
            },
            other => other,
        })?;
    let mut sections = Vec::with_capacity(usize::from(count));
    let mut section_offset = table_offset;
    for section_index in 0..count {
        let (record, next_offset) = reader.read(section_offset, 40)?;
        let mut name = [0; 8];
        name.copy_from_slice(&record[..8]);
        let virtual_size = read_u32(record, 8);
        let virtual_address = RelativeVirtualAddress::new(read_u32(record, 12));
        let virtual_end = u64::from(virtual_address.get()) + u64::from(virtual_size);
        if virtual_end > 1_u64 << 32 {
            return Err(PeHeaderError::VirtualRangeOverflow {
                section_index,
                offset: section_offset,
                virtual_address,
                virtual_size,
            });
        }
        let size_of_raw_data = read_u32(record, 16);
        let pointer_to_raw_data = FileOffset::new(u64::from(read_u32(record, 20)));
        let raw_data = if size_of_raw_data == 0 {
            &bytes[..0]
        } else {
            reader
                .read(pointer_to_raw_data, u64::from(size_of_raw_data))
                .map_err(|error| match error {
                    PeHeaderError::OutOfBounds {
                        offset,
                        needed,
                        available,
                    } => PeHeaderError::SectionRawDataOutOfBounds {
                        section_index,
                        section_offset,
                        offset,
                        needed,
                        available,
                    },
                    other => other,
                })?
                .0
        };
        sections.push(PeSection {
            name,
            virtual_size,
            virtual_address,
            size_of_raw_data,
            pointer_to_raw_data,
            pointer_to_relocations: FileOffset::new(u64::from(read_u32(record, 24))),
            pointer_to_line_numbers: FileOffset::new(u64::from(read_u32(record, 28))),
            number_of_relocations: read_u16(record, 32),
            number_of_line_numbers: read_u16(record, 34),
            characteristics: read_u32(record, 36),
            raw_data,
        });
        section_offset = next_offset;
    }
    Ok(PeSectionTable { headers, sections })
}
