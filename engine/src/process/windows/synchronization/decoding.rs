use crate::{
    memory::{Access, AddressSpace, GuestAddress},
    process::HostError,
    windows::synchronization::SynchronizationError32 as Error,
};

pub(super) fn name(memory: &AddressSpace, address: u32, wide: bool) -> Result<Vec<u16>, HostError> {
    let mut result = Vec::new();
    result
        .try_reserve_exact(260)
        .map_err(|_| HostError::Synchronization(Error::Allocation))?;
    for index in 0..=260 {
        let address = address
            .checked_add(index * if wide { 2 } else { 1 })
            .ok_or(HostError::Synchronization(Error::InvalidName))?;
        let mut bytes = [0; 2];
        memory
            .read(
                GuestAddress(address),
                &mut bytes[..if wide { 2 } else { 1 }],
            )
            .map_err(HostError::Memory)?;
        let unit = if wide {
            u16::from_le_bytes(bytes)
        } else {
            ansi(bytes[0])
        };
        if unit == 0 {
            return Ok(result);
        }
        if index == 260 {
            return Err(HostError::Synchronization(Error::InvalidName));
        }
        result.push(unit);
    }
    unreachable!()
}
pub(super) fn handles(
    memory: &AddressSpace,
    address: u32,
    count: u32,
) -> Result<([u32; 64], usize), HostError> {
    if count > 64 {
        return Err(HostError::Synchronization(Error::InvalidWait));
    }
    let mut result = [0; 64];
    for (index, handle) in result[..count as usize].iter_mut().enumerate() {
        *handle = words::<1>(
            memory,
            address
                .checked_add(index as u32 * 4)
                .ok_or(HostError::Synchronization(Error::InvalidWait))?,
        )?[0];
    }
    Ok((result, count as usize))
}
pub(super) fn inherit(memory: &AddressSpace, address: u32) -> Result<bool, HostError> {
    if address == 0 {
        return Ok(false);
    }
    let [size, descriptor, inherit] = words::<3>(memory, address)?;
    if size != 12 {
        return Err(HostError::Synchronization(Error::InvalidWait));
    }
    if descriptor != 0 {
        return Err(HostError::Synchronization(Error::Unsupported));
    }
    Ok(inherit != 0)
}
pub(super) fn words<const N: usize>(
    memory: &AddressSpace,
    address: u32,
) -> Result<[u32; N], HostError> {
    let mut bytes = [0; 4];
    let mut result = [0; N];
    for (index, word) in result.iter_mut().enumerate() {
        memory
            .read(
                GuestAddress(
                    address
                        .checked_add(index as u32 * 4)
                        .ok_or(HostError::Synchronization(Error::InvalidWait))?,
                ),
                &mut bytes,
            )
            .map_err(HostError::Memory)?;
        *word = u32::from_le_bytes(bytes);
    }
    Ok(result)
}
pub(super) fn check_output(memory: &AddressSpace, output: u32) -> Result<(), HostError> {
    if output != 0 {
        memory
            .check_span(GuestAddress(output), 4, Access::Write)
            .map_err(HostError::Memory)?;
    }
    Ok(())
}
fn ansi(byte: u8) -> u16 {
    const EXTENDED: [u16; 32] = [
        0x20ac, 0x81, 0x201a, 0x192, 0x201e, 0x2026, 0x2020, 0x2021, 0x2c6, 0x2030, 0x160, 0x2039,
        0x152, 0x8d, 0x17d, 0x8f, 0x90, 0x2018, 0x2019, 0x201c, 0x201d, 0x2022, 0x2013, 0x2014,
        0x2dc, 0x2122, 0x161, 0x203a, 0x153, 0x9d, 0x17e, 0x178,
    ];
    if (0x80..=0x9f).contains(&byte) {
        EXTENDED[(byte - 0x80) as usize]
    } else {
        u16::from(byte)
    }
}
