use super::{PeHeaderError, PeHeaders, PeSectionTable, Reader, parse_pe_sections};
use crate::{FileOffset, RelativeVirtualAddress};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeFileRangeSource {
    Headers,
    Section(u16),
}

/// bytes from one unambiguous file-backed region; not a mapped image.
///
/// ```compile_fail
/// use ring3_core::{PeFileRange, RelativeVirtualAddress, resolve_pe_file_range};
///
/// fn escape() -> PeFileRange<'static> {
///     let bytes = vec![0; 64];
///     resolve_pe_file_range(&bytes, RelativeVirtualAddress::new(0), 2).unwrap()
/// }
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeFileRange<'a> {
    pub file_offset: FileOffset,
    pub bytes: &'a [u8],
    pub source: PeFileRangeSource,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeRvaError {
    EmptyRange {
        start: RelativeVirtualAddress,
    },
    RvaRangeOverflow {
        start: RelativeVirtualAddress,
        length: u32,
    },
    Parse(PeHeaderError),
    InvalidHeaderExtent {
        size_of_headers: u32,
        minimum: u64,
        file_size: u64,
    },
    AmbiguousRange {
        start: RelativeVirtualAddress,
        length: u32,
    },
    UnmappedRva {
        start: RelativeVirtualAddress,
        length: u32,
    },
    CrossesRegionBoundary {
        start: RelativeVirtualAddress,
        length: u32,
    },
    ZeroVirtualSizeUnsupported {
        start: RelativeVirtualAddress,
        length: u32,
        section_index: u16,
    },
    RawPaddingUnsupported {
        start: RelativeVirtualAddress,
        length: u32,
        section_index: u16,
    },
    NotFileBacked {
        start: RelativeVirtualAddress,
        length: u32,
        section_index: u16,
    },
    FileRangeOutOfBounds {
        start: RelativeVirtualAddress,
        length: u32,
        file_offset: FileOffset,
        available: u64,
    },
}

#[derive(Clone, Copy)]
struct Region {
    start: u64,
    end: u64,
    source: PeFileRangeSource,
}

fn regions<'a>(table: &'a PeSectionTable<'_>) -> impl Iterator<Item = Region> + 'a {
    std::iter::once(Region {
        start: 0,
        end: u64::from(table.headers.optional.size_of_headers),
        source: PeFileRangeSource::Headers,
    })
    .chain((0_u16..).zip(&table.sections).map(|(index, section)| {
        let start = u64::from(section.virtual_address.get());
        let extent = u64::from(section.virtual_size.max(section.size_of_raw_data));
        Region {
            start,
            end: (start + extent).min(1_u64 << 32),
            source: PeFileRangeSource::Section(index),
        }
    }))
}

fn select_region(
    table: &PeSectionTable<'_>,
    start: RelativeVirtualAddress,
    length: u32,
) -> Result<Region, PeRvaError> {
    let query_start = u64::from(start.get());
    let query_end = query_start + u64::from(length);
    let mut intersects = false;
    let mut containing = None;
    for (index, region) in regions(table).enumerate() {
        if query_start.max(region.start) >= query_end.min(region.end) {
            continue;
        }
        intersects = true;
        for other in regions(table).skip(index + 1) {
            if query_start.max(region.start).max(other.start)
                < query_end.min(region.end).min(other.end)
            {
                return Err(PeRvaError::AmbiguousRange { start, length });
            }
        }
        if region.start <= query_start && query_end <= region.end {
            containing = Some(region);
        }
    }
    containing.ok_or(if intersects {
        PeRvaError::CrossesRegionBoundary { start, length }
    } else {
        PeRvaError::UnmappedRva { start, length }
    })
}

fn request_end(start: RelativeVirtualAddress, length: u32) -> Result<u64, PeRvaError> {
    if length == 0 {
        return Err(PeRvaError::EmptyRange { start });
    }
    let query_start = u64::from(start.get());
    let query_end = query_start + u64::from(length);
    if query_end > 1_u64 << 32 {
        return Err(PeRvaError::RvaRangeOverflow { start, length });
    }
    Ok(query_end)
}

pub(super) struct PreparedPe<'a> {
    bytes: &'a [u8],
    table: PeSectionTable<'a>,
}

impl<'a> PreparedPe<'a> {
    pub(super) fn new(bytes: &'a [u8]) -> Result<Self, PeRvaError> {
        let table = parse_pe_sections(bytes).map_err(PeRvaError::Parse)?;
        let prefix = table.headers.prefix;
        let minimum = (prefix.pe_offset.get()
            + 24
            + u64::from(prefix.size_of_optional_header)
            + u64::from(prefix.number_of_sections) * 40)
            .max(64);
        let size_of_headers = table.headers.optional.size_of_headers;
        if u64::from(size_of_headers) < minimum || u64::from(size_of_headers) > bytes.len() as u64 {
            return Err(PeRvaError::InvalidHeaderExtent {
                size_of_headers,
                minimum,
                file_size: bytes.len() as u64,
            });
        }
        Ok(Self { bytes, table })
    }

    pub(super) fn headers(&self) -> &PeHeaders {
        &self.table.headers
    }

    pub(super) fn resolve(
        &self,
        start: RelativeVirtualAddress,
        length: u32,
    ) -> Result<PeFileRange<'a>, PeRvaError> {
        let query_end = request_end(start, length)?;
        let query_start = u64::from(start.get());
        let region = select_region(&self.table, start, length)?;
        let file_offset = match region.source {
            PeFileRangeSource::Headers => FileOffset::new(query_start),
            PeFileRangeSource::Section(section_index) => {
                let section = &self.table.sections[usize::from(section_index)];
                if section.virtual_size == 0 {
                    return Err(PeRvaError::ZeroVirtualSizeUnsupported {
                        start,
                        length,
                        section_index,
                    });
                }
                let relative_end = query_end - region.start;
                if relative_end > u64::from(section.virtual_size) {
                    return Err(PeRvaError::RawPaddingUnsupported {
                        start,
                        length,
                        section_index,
                    });
                }
                if relative_end > u64::from(section.size_of_raw_data) {
                    return Err(PeRvaError::NotFileBacked {
                        start,
                        length,
                        section_index,
                    });
                }
                FileOffset::new(section.pointer_to_raw_data.get() + query_start - region.start)
            }
        };
        let (resolved, _) = Reader { bytes: self.bytes }
            .read(file_offset, u64::from(length))
            .map_err(|error| match error {
                PeHeaderError::OutOfBounds { available, .. } => PeRvaError::FileRangeOutOfBounds {
                    start,
                    length,
                    file_offset,
                    available,
                },
                other => PeRvaError::Parse(other),
            })?;
        Ok(PeFileRange {
            file_offset,
            bytes: resolved,
            source: region.source,
        })
    }
}

/// resolves one complete range under conservative header and section policies.
/// headers must cover the declared section table and fit the input. sections
/// expose only the smaller virtual/raw extent; the larger extent still guards
/// against ambiguous selection and unsupported tails. overlaps matter only
/// inside the request. ranges are never stitched and zero-fill is not generated.
///
/// # errors
/// rejects invalid requests, parse/header failures, requested-byte ambiguity,
/// region crossings, unsupported tails, and missing physical bytes, in that order.
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn resolve_pe_file_range(
    bytes: &[u8],
    start: RelativeVirtualAddress,
    length: u32,
) -> Result<PeFileRange<'_>, PeRvaError> {
    request_end(start, length)?;
    PreparedPe::new(bytes)?.resolve(start, length)
}
