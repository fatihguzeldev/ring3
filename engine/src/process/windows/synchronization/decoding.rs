use crate::{
    memory::{Access, AddressSpace, GuestAddress},
    process::HostError,
    windows::synchronization::SynchronizationError32 as Error,
};

pub(super) fn name(
    memory: &AddressSpace,
    address: u32,
    wide: bool,
    code_page: crate::windows::text::CodePage32,
) -> Result<Vec<u16>, HostError> {
    let mut result = Vec::new();
    result
        .try_reserve_exact(260)
        .map_err(|_| HostError::Synchronization(Error::Allocation))?;
    let mut ansi = Vec::new();
    let maximum = if wide { 260 } else { 1040 };
    for index in 0..=maximum {
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
            u16::from(bytes[0])
        };
        if unit == 0 {
            if !wide {
                result = code_page
                    .decode(&ansi, false)
                    .map_err(|_| HostError::Synchronization(Error::InvalidName))?;
            }
            return if result.len() <= 260 {
                Ok(result)
            } else {
                Err(HostError::Synchronization(Error::InvalidName))
            };
        }
        if index == maximum {
            return Err(HostError::Synchronization(Error::InvalidName));
        }
        if wide {
            result.push(unit);
        } else {
            ansi.try_reserve(1)
                .map_err(|_| HostError::Synchronization(Error::Allocation))?;
            ansi.push(bytes[0]);
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        memory::{PageRange, Permissions},
        windows::text::CodePage32,
    };

    #[test]
    fn ansi_names_decode_complete_multibyte_sequences_from_the_configured_page() {
        let mut memory = AddressSpace::new(2).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                Permissions::READ_WRITE,
            )
            .unwrap();
        memory
            .write(
                GuestAddress(0x1000),
                &[0x83, 0x65, 0x83, 0x58, 0x83, 0x67, 0],
            )
            .unwrap();
        assert_eq!(
            name(&memory, 0x1000, false, CodePage32::from_id(932).unwrap()).unwrap(),
            "テスト".encode_utf16().collect::<Vec<_>>()
        );
        assert_ne!(
            name(&memory, 0x1000, false, CodePage32::Windows1252).unwrap(),
            "テスト".encode_utf16().collect::<Vec<_>>()
        );
        let mut bytes = [0x83, 0x65].repeat(260);
        bytes.push(0);
        memory.write(GuestAddress(0x1000), &bytes).unwrap();
        assert_eq!(
            name(&memory, 0x1000, false, CodePage32::from_id(932).unwrap())
                .unwrap()
                .len(),
            260
        );
        bytes.truncate(520);
        bytes.extend_from_slice(&[0x83, 0x65, 0]);
        memory.write(GuestAddress(0x1000), &bytes).unwrap();
        assert_eq!(
            name(&memory, 0x1000, false, CodePage32::from_id(932).unwrap()),
            Err(HostError::Synchronization(Error::InvalidName))
        );
    }
}
