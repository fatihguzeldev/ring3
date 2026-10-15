use super::{
    PreparedImagePage32, PreparedPe32,
    metadata::Relocation32,
    read::{align, reserve},
};
use crate::{
    loader::LoadError,
    memory::{Access, GuestAddress, MemoryError, Permissions},
};

impl PreparedPe32 {
    pub(crate) fn prepare_pages(
        &self,
        source: &[u8],
        actual_base: u32,
    ) -> Result<Vec<PreparedImagePage32>, LoadError> {
        if source.len() != self.source_length
            || actual_base == 0
            || !actual_base.is_multiple_of(65536)
            || u64::from(actual_base) + align(u64::from(self.image_size), 4096) > 1 << 32
        {
            return Err(LoadError::Malformed);
        }
        let delta = actual_base.wrapping_sub(self.preferred_base);
        if delta != 0 && (self.relocs_stripped || self.relocation_directory.is_none()) {
            return Err(LoadError::Unsupported);
        }
        let mut indices = Vec::new();
        reserve(
            &mut indices,
            self.mapped_pages as usize + self.sections.len(),
        )?;
        indices.extend(0..(align(u64::from(self.header_size), 4096) / 4096) as u32);
        for section in &self.sections {
            let span = section.page_span();
            indices.extend((span.start / 4096) as u32..(span.end / 4096) as u32);
        }
        indices.sort_unstable();
        indices.dedup();
        let mut pages = Vec::new();
        reserve(&mut pages, indices.len())?;
        for index in indices {
            let mut data = Vec::new();
            data.try_reserve_exact(4096)
                .map_err(|_| LoadError::Memory(MemoryError::Allocation))?;
            data.resize(4096, 0);
            let bytes = data
                .into_boxed_slice()
                .try_into()
                .map_err(|_| LoadError::Malformed)?;
            let address = GuestAddress(actual_base + index * 4096);
            pages.push(PreparedImagePage32 {
                address,
                permissions: Permissions::NONE,
                bytes,
            });
        }
        copy_into(
            &mut pages,
            actual_base,
            &source[..self.header_size as usize],
        )?;
        set_permissions(
            &mut pages,
            actual_base,
            0,
            align(u64::from(self.header_size), 4096),
            Permissions::READ,
        )?;
        for section in &self.sections {
            if !section.raw.is_empty() {
                copy_into(
                    &mut pages,
                    actual_base + section.rva,
                    &source[section.raw.clone()],
                )?;
            }
            let span = section.page_span();
            set_permissions(
                &mut pages,
                actual_base,
                span.start,
                span.end,
                section.permissions,
            )?;
        }
        for relocation in &self.relocations {
            let (rva, length) = relocation.span();
            let address = actual_base + rva;
            let mut value = [0; 4];
            copy_out(&pages, address, &mut value[..length as usize])?;
            let half = u16::from_le_bytes([value[0], value[1]]);
            match *relocation {
                Relocation32::High(_) => value[..2]
                    .copy_from_slice(&half.wrapping_add((delta >> 16) as u16).to_le_bytes()),
                Relocation32::Low(_) => {
                    value[..2].copy_from_slice(&half.wrapping_add(delta as u16).to_le_bytes())
                }
                Relocation32::HighLow(_) => {
                    value = u32::from_le_bytes(value).wrapping_add(delta).to_le_bytes()
                }
                Relocation32::HighAdj(_, low) => {
                    let high = u32::from(half) << 16;
                    let adjusted = high
                        .wrapping_add(low as i32 as u32)
                        .wrapping_add(delta)
                        .wrapping_add(0x8000);
                    value[..2].copy_from_slice(&((adjusted >> 16) as u16).to_le_bytes());
                }
            }
            copy_into(&mut pages, address, &value[..length as usize])?;
        }
        Ok(pages)
    }
}

fn set_permissions(
    pages: &mut [PreparedImagePage32],
    base: u32,
    start: u64,
    end: u64,
    permissions: Permissions,
) -> Result<(), LoadError> {
    let mask = |value: Permissions| {
        u8::from(value.allows(Access::Read))
            | (u8::from(value.allows(Access::Write)) << 1)
            | (u8::from(value.allows(Access::Execute)) << 2)
    };
    for rva in (start..end).step_by(4096) {
        let at = base + rva as u32;
        let index = pages
            .binary_search_by_key(&at, |page| page.address.0)
            .map_err(|_| LoadError::Malformed)?;
        pages[index].permissions =
            Permissions::from_bits(mask(pages[index].permissions) | mask(permissions))
                .ok_or(LoadError::Malformed)?;
    }
    Ok(())
}

pub(crate) fn copy_into(
    pages: &mut [PreparedImagePage32],
    mut address: u32,
    mut bytes: &[u8],
) -> Result<(), LoadError> {
    while !bytes.is_empty() {
        let base = address & !4095;
        let index = pages
            .binary_search_by_key(&base, |page| page.address.0)
            .map_err(|_| LoadError::Malformed)?;
        let offset = (address & 4095) as usize;
        let length = bytes.len().min(4096 - offset);
        pages[index].bytes[offset..offset + length].copy_from_slice(&bytes[..length]);
        bytes = &bytes[length..];
        if !bytes.is_empty() {
            address = address
                .checked_add(length as u32)
                .ok_or(LoadError::Malformed)?;
        }
    }
    Ok(())
}

pub(crate) fn copy_out(
    pages: &[PreparedImagePage32],
    mut address: u32,
    mut bytes: &mut [u8],
) -> Result<(), LoadError> {
    while !bytes.is_empty() {
        let base = address & !4095;
        let index = pages
            .binary_search_by_key(&base, |page| page.address.0)
            .map_err(|_| LoadError::Malformed)?;
        let offset = (address & 4095) as usize;
        let length = bytes.len().min(4096 - offset);
        bytes[..length].copy_from_slice(&pages[index].bytes[offset..offset + length]);
        bytes = &mut bytes[length..];
        if !bytes.is_empty() {
            address = address
                .checked_add(length as u32)
                .ok_or(LoadError::Malformed)?;
        }
    }
    Ok(())
}
