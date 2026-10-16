use super::{super::catalog::ModuleSource32, DataFileLayout32};
use crate::{
    loader::{ModuleError32, PreparedImagePage32},
    memory::{GuestAddress, PAGE_SIZE, PageRange, Permissions},
};

pub(super) fn pages(
    source: &ModuleSource32,
    base: u32,
    layout: DataFileLayout32,
) -> Result<(Vec<PreparedImagePage32>, Vec<PageRange>), ModuleError32> {
    let ModuleSource32::Guest { image, bytes, .. } = source else {
        return Err(ModuleError32::InvalidModuleKind);
    };
    let mut indices = Vec::new();
    if layout == DataFileLayout32::File {
        let count = bytes.len().div_ceil(PAGE_SIZE as usize);
        indices
            .try_reserve_exact(count)
            .map_err(|_| ModuleError32::Allocation)?;
        indices.extend(0..count as u32);
    } else {
        let maximum = (image.mapped_pages as usize)
            .checked_add(image.sections.len())
            .ok_or(ModuleError32::Capacity)?;
        indices
            .try_reserve_exact(maximum)
            .map_err(|_| ModuleError32::Allocation)?;
        indices.extend(0..image.header_size.div_ceil(PAGE_SIZE));
        for section in &image.sections {
            let size = u64::from(section.virtual_size).max(section.raw.len() as u64);
            if size != 0 {
                indices.extend(
                    section.rva / PAGE_SIZE
                        ..((u64::from(section.rva) + size).div_ceil(u64::from(PAGE_SIZE))) as u32,
                );
            }
        }
        indices.sort_unstable();
        indices.dedup();
    }
    let mut pages = Vec::new();
    let mut ranges: Vec<PageRange> = Vec::new();
    pages
        .try_reserve_exact(indices.len())
        .map_err(|_| ModuleError32::Allocation)?;
    ranges
        .try_reserve_exact(indices.len())
        .map_err(|_| ModuleError32::Allocation)?;
    for index in indices {
        let mut data = Vec::new();
        data.try_reserve_exact(PAGE_SIZE as usize)
            .map_err(|_| ModuleError32::Allocation)?;
        data.resize(PAGE_SIZE as usize, 0);
        let start = index as usize * PAGE_SIZE as usize;
        if layout == DataFileLayout32::File {
            let end = (start + PAGE_SIZE as usize).min(bytes.len());
            data[..end - start].copy_from_slice(&bytes[start..end]);
        } else {
            let end = (start + PAGE_SIZE as usize).min(image.header_size as usize);
            if start < end {
                data[..end - start].copy_from_slice(&bytes[start..end]);
            }
            for section in &image.sections {
                let from = start.max(section.rva as usize);
                let end =
                    (start + PAGE_SIZE as usize).min(section.rva as usize + section.raw.len());
                if from < end {
                    let raw = section.raw.start + from - section.rva as usize;
                    data[from - start..end - start].copy_from_slice(&bytes[raw..raw + end - from]);
                }
            }
        }
        let address = GuestAddress(base + index * PAGE_SIZE);
        pages.push(PreparedImagePage32 {
            address,
            permissions: Permissions::READ,
            bytes: data
                .into_boxed_slice()
                .try_into()
                .map_err(|_| ModuleError32::InvalidRequest)?,
        });
        if let Some(last) = ranges
            .last_mut()
            .filter(|last| last.first + last.count == address.0 as usize / PAGE_SIZE as usize)
        {
            last.count += 1;
        } else {
            ranges.push(PageRange::new(address, 1)?);
        }
    }
    Ok((pages, ranges))
}
