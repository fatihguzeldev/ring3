use crate::{
    loader::ResourceIdentifier32,
    memory::{Access, AddressSpace, GuestAddress},
    windows::{activation::ActivationError32 as E, text::CodePage32},
};

pub(super) fn read<const N: usize>(memory: &AddressSpace, address: u32) -> Result<[u8; N], E> {
    if address == 0 {
        return Err(E::NoAccess);
    }
    let mut bytes = [0; N];
    memory
        .read(GuestAddress(address), &mut bytes)
        .map_err(|_| E::NoAccess)?;
    Ok(bytes)
}
pub(super) fn output(memory: &AddressSpace, address: u32, length: usize) -> Result<(), E> {
    if address == 0 {
        return Err(E::NoAccess);
    }
    memory
        .check_span(GuestAddress(address), length, Access::Write)
        .map_err(|_| E::NoAccess)
}
pub(super) fn string(
    memory: &AddressSpace,
    address: u32,
    wide: bool,
    cp: CodePage32,
) -> Result<Vec<u16>, E> {
    if address == 0 {
        return Err(E::InvalidParameter);
    }
    let mut units = Vec::new();
    let mut bytes = Vec::new();
    let maximum = 32767usize;
    let raw = maximum
        * if wide {
            1
        } else {
            cp.maximum_char_size() as usize
        };
    for index in 0..=raw {
        let at = address
            .checked_add(index as u32 * if wide { 2 } else { 1 })
            .ok_or(E::NoAccess)?;
        if wide {
            let unit = u16::from_le_bytes(read(memory, at)?);
            if unit == 0 {
                return Ok(units);
            }
            units.try_reserve(1).map_err(|_| E::NotEnoughMemory)?;
            units.push(unit);
        } else {
            let byte = read::<1>(memory, at)?[0];
            if byte == 0 {
                let units = cp.decode(&bytes, false).map_err(|_| E::InvalidParameter)?;
                return if units.len() <= maximum {
                    Ok(units)
                } else {
                    Err(E::InvalidParameter)
                };
            }
            bytes.try_reserve(1).map_err(|_| E::NotEnoughMemory)?;
            bytes.push(byte);
        }
    }
    Err(E::InvalidParameter)
}
pub(super) fn identifier(
    memory: &AddressSpace,
    address: u32,
    wide: bool,
    cp: CodePage32,
) -> Result<ResourceIdentifier32, E> {
    if address <= u32::from(u16::MAX) {
        Ok(ResourceIdentifier32::Id(address))
    } else {
        string(memory, address, wide, cp).map(ResourceIdentifier32::Name)
    }
}
