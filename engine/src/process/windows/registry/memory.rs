use crate::{
    memory::{Access, AddressSpace, GuestAddress},
    windows::{registry::RegistryError32 as E, text::CodePage32},
};
pub(super) fn read<const N: usize>(memory: &AddressSpace, address: u32) -> Result<[u8; N], E> {
    if address == 0 {
        return Err(E::InvalidParameter);
    }
    let mut bytes = [0; N];
    memory
        .read(GuestAddress(address), &mut bytes)
        .map_err(|_| E::NoAccess)?;
    Ok(bytes)
}
pub(super) fn dword(memory: &AddressSpace, address: u32) -> Result<u32, E> {
    Ok(u32::from_le_bytes(read(memory, address)?))
}
pub(super) fn output(memory: &AddressSpace, address: u32, length: usize) -> Result<(), E> {
    if address == 0 {
        return Err(E::InvalidParameter);
    }
    memory
        .check_span(GuestAddress(address), length, Access::Write)
        .map_err(|_| E::NoAccess)
}
pub(super) fn input(
    memory: &AddressSpace,
    address: u32,
    length: usize,
    maximum: usize,
) -> Result<Vec<u8>, E> {
    if length > maximum {
        return Err(E::NotEnoughMemory);
    }
    if length == 0 {
        return Ok(Vec::new());
    }
    if address == 0 {
        return Err(E::InvalidParameter);
    }
    memory
        .check_span(GuestAddress(address), length, Access::Read)
        .map_err(|_| E::NoAccess)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| E::NotEnoughMemory)?;
    bytes.resize(length, 0);
    memory
        .read(GuestAddress(address), &mut bytes)
        .map_err(|_| E::NoAccess)?;
    Ok(bytes)
}
pub(super) fn string(
    memory: &AddressSpace,
    address: u32,
    wide: bool,
    maximum: usize,
    cp: CodePage32,
) -> Result<Vec<u16>, E> {
    if address == 0 {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    if wide {
        for index in 0..=maximum {
            let at = address
                .checked_add((index as u32).checked_mul(2).ok_or(E::InvalidParameter)?)
                .ok_or(E::InvalidParameter)?;
            let unit = u16::from_le_bytes(read(memory, at)?);
            if unit == 0 {
                return Ok(result);
            }
            result.try_reserve(1).map_err(|_| E::NotEnoughMemory)?;
            result.push(unit);
        }
    } else {
        let mut bytes = Vec::new();
        let maximum_bytes = maximum
            .checked_mul(cp.maximum_char_size() as usize)
            .ok_or(E::InvalidParameter)?;
        for index in 0..=maximum_bytes {
            let byte = read::<1>(
                memory,
                address
                    .checked_add(index as u32)
                    .ok_or(E::InvalidParameter)?,
            )?[0];
            if byte == 0 {
                let units = cp.decode(&bytes, false).map_err(|_| E::InvalidData)?;
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
pub(super) fn encoded_name(
    units: &[u16],
    wide: bool,
    terminated: bool,
    cp: CodePage32,
) -> Result<Vec<u8>, E> {
    let mut bytes = if wide {
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(units.len() * 2 + usize::from(terminated) * 2)
            .map_err(|_| E::NotEnoughMemory)?;
        for unit in units {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes
    } else {
        cp.encode(units, false).map_err(|_| E::InvalidData)?.0
    };
    if terminated {
        bytes.push(0);
        if wide {
            bytes.push(0);
        }
    }
    Ok(bytes)
}
pub(super) fn value_guest_length(
    kind: u32,
    bytes: &[u8],
    wide: bool,
    cp: CodePage32,
) -> Result<usize, E> {
    if wide || !matches!(kind, 1 | 2 | 7) {
        return Ok(bytes.len());
    }
    if !bytes.len().is_multiple_of(2) {
        return Err(E::InvalidData);
    }
    Ok(value_to_guest(kind, bytes, wide, cp)?.len())
}
pub(super) fn value_to_guest(
    kind: u32,
    bytes: &[u8],
    wide: bool,
    cp: CodePage32,
) -> Result<Vec<u8>, E> {
    if wide || !matches!(kind, 1 | 2 | 7) {
        let mut copy = Vec::new();
        copy.try_reserve_exact(bytes.len())
            .map_err(|_| E::NotEnoughMemory)?;
        copy.extend_from_slice(bytes);
        return Ok(copy);
    }
    if !bytes.len().is_multiple_of(2) {
        return Err(E::InvalidData);
    }
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes(bytes.try_into().unwrap()))
        .collect();
    cp.encode(&units, false)
        .map(|(bytes, _)| bytes)
        .map_err(|_| E::InvalidData)
}
pub(super) fn value_from_guest(
    kind: u32,
    bytes: Vec<u8>,
    wide: bool,
    cp: CodePage32,
) -> Result<Vec<u8>, E> {
    if wide || !matches!(kind, 1 | 2 | 7) {
        return Ok(bytes);
    }
    let units = cp.decode(&bytes, false).map_err(|_| E::InvalidData)?;
    encoded_name(&units, true, false, cp)
}
pub(super) fn publish(
    memory: &mut AddressSpace,
    writes: &[(GuestAddress, &[u8])],
) -> Result<(), E> {
    memory.write_spans(writes).map_err(|_| E::NoAccess)
}
