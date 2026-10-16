use super::{ActivationSection32, ActivationSectionEntry32, ActivationSectionKey32};
use crate::windows::activation::ActivationError32 as E;

pub(super) struct Builder32 {
    pub(super) section: ActivationSection32,
    count: usize,
    guid: bool,
    limit: usize,
}
impl Builder32 {
    pub(super) fn new(id: u32, count: usize, guid: bool, limit: usize) -> Result<Self, E> {
        let header = if guid { 40usize } else { 44 };
        let index = if guid { 28usize } else { 24 };
        let length = count
            .checked_mul(index)
            .and_then(|length| length.checked_add(header))
            .ok_or(E::NotEnoughMemory)?;
        if length > limit || length > u32::MAX as usize {
            return Err(E::NotEnoughMemory);
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(length)
            .map_err(|_| E::NotEnoughMemory)?;
        bytes.resize(length, 0);
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(count)
            .map_err(|_| E::NotEnoughMemory)?;
        let mut result = Self {
            section: ActivationSection32 {
                id,
                bytes,
                entries,
                global_offset: 0,
                global_length: 0,
            },
            count,
            guid,
            limit,
        };
        result.word(0, if guid { 0x64487347 } else { 0x64487353 });
        result.word(4, header as u32);
        result.word(20, count as u32);
        result.word(24, header as u32);
        Ok(result)
    }
    pub(super) fn word(&mut self, offset: usize, value: u32) {
        self.section.bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    pub(super) fn bytes(&mut self, bytes: &[u8]) -> Result<u32, E> {
        let offset = self.section.bytes.len();
        let end = offset
            .checked_add(bytes.len())
            .filter(|&end| end <= self.limit && end <= u32::MAX as usize)
            .ok_or(E::NotEnoughMemory)?;
        self.section
            .bytes
            .try_reserve(end - offset)
            .map_err(|_| E::NotEnoughMemory)?;
        self.section.bytes.extend_from_slice(bytes);
        Ok(offset as u32)
    }
    pub(super) fn zeroes(&mut self, length: usize) -> Result<u32, E> {
        let offset = self.section.bytes.len();
        let end = offset
            .checked_add(length)
            .filter(|&end| end <= self.limit && end <= u32::MAX as usize)
            .ok_or(E::NotEnoughMemory)?;
        self.section
            .bytes
            .try_reserve(length)
            .map_err(|_| E::NotEnoughMemory)?;
        self.section.bytes.resize(end, 0);
        Ok(offset as u32)
    }
    pub(super) fn align(&mut self) -> Result<(), E> {
        self.zeroes((4 - self.section.bytes.len() % 4) % 4)?;
        Ok(())
    }
    pub(super) fn text(&mut self, text: &str) -> Result<(u32, u32), E> {
        let units = text.encode_utf16().count();
        let length = units.checked_mul(2).ok_or(E::NotEnoughMemory)?;
        let offset = self.zeroes(length.checked_add(2).ok_or(E::NotEnoughMemory)?)?;
        for (index, unit) in text.encode_utf16().enumerate() {
            self.section.bytes[offset as usize + index * 2..offset as usize + index * 2 + 2]
                .copy_from_slice(&unit.to_le_bytes());
        }
        Ok((offset, length as u32))
    }
    pub(super) fn global(&mut self, offset: u32, length: u32) {
        self.section.global_offset = offset;
        self.section.global_length = length;
        self.word(if self.guid { 32 } else { 36 }, offset);
        self.word(if self.guid { 36 } else { 40 }, length);
    }
    pub(super) fn entry(
        &mut self,
        key: ActivationSectionKey32,
        data: u32,
        length: u32,
        roster: u32,
    ) -> Result<(), E> {
        let index = self.section.entries.len();
        if index >= self.count {
            return Err(E::InvalidParameter);
        }
        let at = if self.guid {
            40 + index * 28
        } else {
            44 + index * 24
        };
        match &key {
            ActivationSectionKey32::Guid(guid) if self.guid => {
                self.section.bytes[at..at + 16].copy_from_slice(guid);
                self.word(at + 16, data);
                self.word(at + 20, length);
                self.word(at + 24, roster);
            }
            ActivationSectionKey32::String(units) if !self.guid => {
                let text = String::from_utf16(units).map_err(|_| E::ManifestFormat)?;
                let (offset, bytes) = self.text(&text)?;
                let hash = crate::windows::text::ordinal_key(units)
                    .into_iter()
                    .fold(0u32, |hash, unit| {
                        hash.wrapping_mul(65599).wrapping_add(u32::from(unit))
                    });
                self.word(at, hash);
                self.word(at + 4, offset);
                self.word(at + 8, bytes);
                self.word(at + 12, data);
                self.word(at + 16, length);
                self.word(at + 20, roster);
            }
            _ => return Err(E::InvalidParameter),
        }
        self.section.entries.push(ActivationSectionEntry32 {
            key,
            offset: data,
            length,
            roster,
        });
        Ok(())
    }
    pub(super) fn finish(self) -> Result<ActivationSection32, E> {
        if self.section.entries.len() != self.count {
            return Err(E::InvalidParameter);
        }
        Ok(self.section)
    }
}
