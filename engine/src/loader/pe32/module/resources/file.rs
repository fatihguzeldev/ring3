use super::ResourceError32 as E;
use crate::loader::Directory32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceFileRange32 {
    pub offset: u64,
    pub length: u32,
}
#[derive(Clone, Debug)]
pub struct ResourceFileSpan32 {
    pub offset: u64,
    pub bytes: Vec<u8>,
}
#[derive(Clone, Copy, Debug)]
struct ResourceSection32 {
    rva: u32,
    virtual_size: u32,
    offset: u32,
    raw_size: u32,
}
#[derive(Clone, Debug)]
pub struct PeResourceFile32 {
    pub(super) image_size: u32,
    pub(super) directory: Directory32,
    header_size: u32,
    file_length: u64,
    sections: Vec<ResourceSection32>,
}
impl PeResourceFile32 {
    pub fn parse(headers: &[u8], file_length: u64) -> Result<Self, E> {
        let read = |offset: usize| -> Result<u32, E> {
            Ok(u32::from_le_bytes(
                headers
                    .get(offset..offset.checked_add(4).ok_or(E::Malformed)?)
                    .ok_or(E::MissingFileRange {
                        offset: offset as u64,
                        length: 4,
                    })?
                    .try_into()
                    .unwrap(),
            ))
        };
        let half = |offset: usize| -> Result<u16, E> {
            Ok(u16::from_le_bytes(
                headers
                    .get(offset..offset.checked_add(2).ok_or(E::Malformed)?)
                    .ok_or(E::MissingFileRange {
                        offset: offset as u64,
                        length: 2,
                    })?
                    .try_into()
                    .unwrap(),
            ))
        };
        if headers.len() < 64 {
            return Err(E::MissingFileRange {
                offset: 0,
                length: 64,
            });
        }
        if &headers[..2] != b"MZ" {
            return Err(E::Malformed);
        }
        let pe = read(0x3c)? as usize;
        if pe < 64 || pe as u64 + 24 > file_length {
            return Err(E::Malformed);
        }
        let signature = read(pe)?;
        if signature != 0x00004550 || half(pe + 4)? != 0x14c {
            return Err(E::Malformed);
        }
        let count = usize::from(half(pe + 6)?);
        if count == 0 || count > 96 {
            return Err(E::Malformed);
        }
        let optional = pe.checked_add(24).ok_or(E::Malformed)?;
        let optional_length = usize::from(half(pe + 20)?);
        let table = optional.checked_add(optional_length).ok_or(E::Malformed)?;
        let table_end = table.checked_add(count * 40).ok_or(E::Malformed)?;
        if optional_length < 120 || table_end as u64 > file_length || half(optional)? != 0x10b {
            return Err(E::Malformed);
        }
        let directory_count = read(optional + 92)? as usize;
        if directory_count > (optional_length - 96) / 8 {
            return Err(E::Malformed);
        }
        if directory_count < 3 {
            return Err(E::TypeNotFound);
        }
        let image_size = read(optional + 56)?;
        let header_size = read(optional + 60)?;
        if image_size == 0
            || (header_size as usize) < table_end
            || u64::from(header_size) > file_length
            || header_size > image_size
        {
            return Err(E::Malformed);
        }
        if table_end > headers.len() {
            return Err(E::MissingFileRange {
                offset: 0,
                length: table_end.try_into().map_err(|_| E::Malformed)?,
            });
        }
        let directory = Directory32 {
            rva: read(optional + 112)?,
            size: read(optional + 116)?,
        };
        if directory.rva == 0 || directory.size == 0 {
            return Err(E::TypeNotFound);
        }
        if directory.size < 16
            || u64::from(directory.rva) + u64::from(directory.size) > u64::from(image_size)
        {
            return Err(E::Malformed);
        }
        let mut sections = Vec::new();
        sections
            .try_reserve_exact(count)
            .map_err(|_| E::Allocation)?;
        for index in 0..count {
            let at = table + index * 40;
            let section = ResourceSection32 {
                virtual_size: read(at + 8)?,
                rva: read(at + 12)?,
                raw_size: read(at + 16)?,
                offset: read(at + 20)?,
            };
            if u64::from(section.rva) + u64::from(section.virtual_size.max(section.raw_size))
                > u64::from(image_size)
                || u64::from(section.offset) + u64::from(section.raw_size) > file_length
                || section.raw_size != 0 && section.offset < header_size
                || sections.iter().any(|old: &ResourceSection32| {
                    let left = u64::from(old.rva)
                        ..u64::from(old.rva) + u64::from(old.virtual_size.max(old.raw_size));
                    let right = u64::from(section.rva)
                        ..u64::from(section.rva)
                            + u64::from(section.virtual_size.max(section.raw_size));
                    left.start < right.end && right.start < left.end
                        || old.raw_size != 0
                            && section.raw_size != 0
                            && u64::from(old.offset)
                                < u64::from(section.offset) + u64::from(section.raw_size)
                            && u64::from(section.offset)
                                < u64::from(old.offset) + u64::from(old.raw_size)
                })
            {
                return Err(E::Malformed);
            }
            sections.push(section);
        }
        let result = Self {
            image_size,
            directory,
            header_size,
            file_length,
            sections,
        };
        result.range(directory.rva, 16)?;
        Ok(result)
    }
    pub fn range(&self, rva: u32, length: u32) -> Result<ResourceFileRange32, E> {
        let end = u64::from(rva) + u64::from(length);
        if end > u64::from(self.image_size) {
            return Err(E::Malformed);
        }
        let offset = if end <= u64::from(self.header_size) {
            u64::from(rva)
        } else {
            let section = self
                .sections
                .iter()
                .find(|section| {
                    rva >= section.rva
                        && end <= u64::from(section.rva) + u64::from(section.raw_size)
                })
                .ok_or(E::Malformed)?;
            u64::from(section.offset) + u64::from(rva - section.rva)
        };
        if offset + u64::from(length) > self.file_length {
            return Err(E::Malformed);
        }
        Ok(ResourceFileRange32 { offset, length })
    }
}
pub(super) struct FileResourceSource32<'a> {
    pub(super) metadata: &'a PeResourceFile32,
    pub(super) spans: &'a [ResourceFileSpan32],
}
impl FileResourceSource32<'_> {
    pub(super) fn read(&self, rva: u32, output: &mut [u8]) -> Result<(), E> {
        let range = self
            .metadata
            .range(rva, output.len().try_into().map_err(|_| E::Malformed)?)?;
        let span = self
            .spans
            .iter()
            .find(|span| {
                span.offset <= range.offset
                    && span
                        .offset
                        .checked_add(span.bytes.len() as u64)
                        .is_some_and(|end| range.offset + u64::from(range.length) <= end)
            })
            .ok_or(E::MissingFileRange {
                offset: range.offset,
                length: range.length,
            })?;
        let start = (range.offset - span.offset) as usize;
        output.copy_from_slice(&span.bytes[start..start + output.len()]);
        Ok(())
    }
}
