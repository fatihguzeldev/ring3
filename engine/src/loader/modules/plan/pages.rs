use crate::{
    loader::{
        ModuleError32, ModuleTls32,
        pe32::module::{PreparedImagePage32, StaticTls32, copy_into, copy_out},
    },
    memory::{GuestAddress, PageRange},
};

fn check_pages(
    pages: &[PreparedImagePage32],
    address: GuestAddress,
    length: usize,
) -> Result<(), ModuleError32> {
    if u64::from(address.0) + length as u64 > 1 << 32 {
        return Err(ModuleError32::InvalidRequest);
    }
    let mut offset = 0;
    while offset < length {
        let current = address.0 + offset as u32;
        pages
            .binary_search_by_key(&(current & !4095), |page| page.address.0)
            .map_err(|_| ModuleError32::AddressConflict)?;
        offset += (4096 - (current & 4095) as usize).min(length - offset);
    }
    Ok(())
}

pub(super) fn read_staged(
    pages: &[PreparedImagePage32],
    address: GuestAddress,
    output: &mut [u8],
) -> Result<(), ModuleError32> {
    check_pages(pages, address, output.len())?;
    copy_out(pages, address.0, output).map_err(ModuleError32::from)
}

pub(super) fn stage_write(
    pages: &mut [PreparedImagePage32],
    address: GuestAddress,
    bytes: &[u8],
) -> Result<(), ModuleError32> {
    check_pages(pages, address, bytes.len())?;
    copy_into(pages, address.0, bytes).map_err(ModuleError32::from)
}

pub(super) fn ranges(pages: &[PreparedImagePage32]) -> Result<Vec<PageRange>, ModuleError32> {
    let mut addresses = Vec::new();
    addresses
        .try_reserve_exact(pages.len())
        .map_err(|_| ModuleError32::Allocation)?;
    addresses.extend(pages.iter().map(|page| page.address.0));
    addresses.sort_unstable();
    let mut ranges = Vec::new();
    ranges
        .try_reserve_exact(addresses.len())
        .map_err(|_| ModuleError32::Allocation)?;
    let mut index = 0;
    while index < addresses.len() {
        let start = addresses[index];
        let mut count = 1;
        while index + count < addresses.len()
            && u64::from(addresses[index + count]) == u64::from(start) + count as u64 * 4096
        {
            count += 1;
        }
        ranges.push(PageRange::new(GuestAddress(start), count as u32)?);
        index += count;
    }
    Ok(ranges)
}

pub(super) fn tls(
    info: &StaticTls32,
    base: u32,
    pages: &[PreparedImagePage32],
) -> Result<ModuleTls32, ModuleError32> {
    let mut template = Vec::new();
    template
        .try_reserve_exact(info.template_size as usize)
        .map_err(|_| ModuleError32::Allocation)?;
    template.resize(info.template_size as usize, 0);
    read_staged(pages, GuestAddress(base + info.template_rva), &mut template)?;
    Ok(ModuleTls32 {
        template,
        zero_fill: info.zero_fill,
        alignment: info.alignment,
        index_address: GuestAddress(base + info.index_rva),
        callbacks: info
            .callbacks
            .iter()
            .map(|rva| GuestAddress(base + rva))
            .collect(),
        callbacks_address: info.callbacks_rva.map(|rva| GuestAddress(base + rva)),
    })
}
