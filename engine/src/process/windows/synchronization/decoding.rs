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
    crate::windows::text::CodePage32::Windows1252.decode_byte(byte).expect("CP1252 is single-byte")
}
