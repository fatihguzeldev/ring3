use super::{ParserLimits32, PreparedPe32, Section32};
use crate::{loader::LoadError, memory::MemoryError};

pub(super) struct ParseBudget {
    entries: usize,
    strings: usize,
}

impl ParseBudget {
    pub(super) fn new(limits: ParserLimits32) -> Self {
        Self {
            entries: limits.metadata_entries,
            strings: limits.string_bytes,
        }
    }

    pub(super) fn entries(&mut self, count: usize) -> Result<(), LoadError> {
        self.entries = self.entries.checked_sub(count).ok_or(LoadError::Capacity)?;
        Ok(())
    }

    fn string_byte(&mut self) -> Result<(), LoadError> {
        self.strings = self.strings.checked_sub(1).ok_or(LoadError::Capacity)?;
        Ok(())
    }
}

pub(super) fn u16(bytes: &[u8], at: usize) -> Result<u16, LoadError> {
    let end = at.checked_add(2).ok_or(LoadError::Malformed)?;
    let value = bytes.get(at..end).ok_or(LoadError::Malformed)?;
    Ok(u16::from_le_bytes([value[0], value[1]]))
}

pub(super) fn u32(bytes: &[u8], at: usize) -> Result<u32, LoadError> {
    let end = at.checked_add(4).ok_or(LoadError::Malformed)?;
    let value = bytes.get(at..end).ok_or(LoadError::Malformed)?;
    Ok(u32::from_le_bytes([value[0], value[1], value[2], value[3]]))
}

pub(super) fn reserve<T>(values: &mut Vec<T>, count: usize) -> Result<(), LoadError> {
    values
        .try_reserve(count)
        .map_err(|_| LoadError::Memory(MemoryError::Allocation))
}

pub(super) fn align(value: u64, alignment: u32) -> u64 {
    value.div_ceil(u64::from(alignment)) * u64::from(alignment)
}

impl Section32 {
    pub(super) fn extent(&self) -> u64 {
        u64::from(self.virtual_size).max(self.raw.len() as u64)
    }

    pub(super) fn page_span(&self) -> std::ops::Range<u64> {
        let start = u64::from(self.rva) / 4096 * 4096;
        let end = if self.extent() == 0 {
            start
        } else {
            align(u64::from(self.rva) + self.extent(), 4096)
        };
        start..end
    }
}

impl PreparedPe32 {
    pub(super) fn mapped_span(&self, rva: u32, length: u32) -> Result<(), LoadError> {
        let end = u64::from(rva) + u64::from(length);
        if end > u64::from(self.image_size) {
            return Err(LoadError::Malformed);
        }
        let mut position = u64::from(rva);
        let header_end = align(u64::from(self.header_size), 4096).min(u64::from(self.image_size));
        if position < header_end {
            position = end.min(header_end);
        }
        for section in &self.sections {
            if position == end {
                break;
            }
            let span = section.page_span();
            if position >= span.start && position < span.end {
                position = end.min(span.end);
            }
        }
        if position == end {
            Ok(())
        } else {
            Err(LoadError::Malformed)
        }
    }

    pub(super) fn source_byte(&self, source: &[u8], rva: u32) -> Result<u8, LoadError> {
        self.mapped_span(rva, 1)?;
        if rva < self.header_size {
            return source
                .get(rva as usize)
                .copied()
                .ok_or(LoadError::Malformed);
        }
        let index = self.sections.partition_point(|section| section.rva <= rva);
        if let Some(section) = index
            .checked_sub(1)
            .and_then(|index| self.sections.get(index))
        {
            let offset = (rva - section.rva) as usize;
            if offset < section.raw.len() {
                return source
                    .get(section.raw.start + offset)
                    .copied()
                    .ok_or(LoadError::Malformed);
            }
        }
        Ok(0)
    }

    pub(super) fn read<const N: usize>(
        &self,
        source: &[u8],
        rva: u32,
    ) -> Result<[u8; N], LoadError> {
        self.mapped_span(rva, N as u32)?;
        let mut result = [0; N];
        for (index, byte) in result.iter_mut().enumerate() {
            *byte = self.source_byte(source, rva + index as u32)?;
        }
        Ok(result)
    }

    pub(super) fn word(&self, source: &[u8], rva: u32) -> Result<u32, LoadError> {
        Ok(u32::from_le_bytes(self.read(source, rva)?))
    }

    pub(super) fn string(
        &self,
        source: &[u8],
        rva: u32,
        end: u32,
        budget: &mut ParseBudget,
    ) -> Result<String, LoadError> {
        if rva == 0 || rva >= end {
            return Err(LoadError::Malformed);
        }
        let mut value = String::new();
        for at in rva..end {
            let byte = self.source_byte(source, at)?;
            if byte == 0 {
                return if value.is_empty() {
                    Err(LoadError::Malformed)
                } else {
                    Ok(value)
                };
            }
            if !byte.is_ascii() {
                return Err(LoadError::Unsupported);
            }
            budget.string_byte()?;
            value
                .try_reserve(1)
                .map_err(|_| LoadError::Memory(MemoryError::Allocation))?;
            value.push(char::from(byte));
        }
        Err(LoadError::Malformed)
    }

    pub(super) fn va_rva(&self, va: u32, length: u32) -> Result<u32, LoadError> {
        let rva = va
            .checked_sub(self.preferred_base)
            .ok_or(LoadError::Malformed)?;
        self.mapped_span(rva, length)?;
        Ok(rva)
    }
}
