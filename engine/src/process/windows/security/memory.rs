use crate::{
    files::FileError32,
    memory::{AddressSpace, GuestAddress},
    windows::security::SecurityDescriptor32,
};

fn read(memory: &AddressSpace, address: u32, length: usize) -> Result<Vec<u8>, FileError32> {
    if address == 0 || u64::from(address) + length as u64 > 1 << 32 {
        return Err(FileError32::NoAccess);
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| FileError32::Allocation)?;
    bytes.resize(length, 0);
    memory
        .read(GuestAddress(address), &mut bytes)
        .map_err(|_| FileError32::NoAccess)?;
    Ok(bytes)
}
pub(in crate::process) fn descriptor(
    memory: &AddressSpace,
    address: u32,
) -> Result<SecurityDescriptor32, FileError32> {
    descriptor_and_length(memory, address).map(|(descriptor, _)| descriptor)
}
pub(super) fn descriptor_and_length(
    memory: &AddressSpace,
    address: u32,
) -> Result<(SecurityDescriptor32, u32), FileError32> {
    components(memory, address, read(memory, address, 20)?)
}
fn components(
    memory: &AddressSpace,
    address: u32,
    mut bytes: Vec<u8>,
) -> Result<(SecurityDescriptor32, u32), FileError32> {
    if bytes[0] != 1 {
        return Err(FileError32::InvalidSecurityDescriptor);
    }
    let control = u16::from_le_bytes(bytes[2..4].try_into().unwrap());
    let relative = control & 0x8000 != 0;
    let mut original_length = 20;
    bytes[2..4].copy_from_slice(&(control | 0x8000).to_le_bytes());
    for (field, present, acl) in [
        (4, true, false),
        (8, true, false),
        (12, control & 16 != 0, true),
        (16, control & 4 != 0, true),
    ] {
        let raw = u32::from_le_bytes(bytes[field..field + 4].try_into().unwrap());
        if !present || raw == 0 {
            bytes[field..field + 4].fill(0);
            continue;
        }
        if relative && (raw < 20 || raw & 3 != 0) {
            return Err(FileError32::InvalidSecurityDescriptor);
        }
        let component = if relative {
            address.checked_add(raw).ok_or(FileError32::NoAccess)?
        } else {
            raw
        };
        let header = read(memory, component, 8)?;
        let length = if acl {
            usize::from(u16::from_le_bytes(header[2..4].try_into().unwrap()))
        } else {
            if header[0] != 1 || header[1] > 15 {
                return Err(FileError32::InvalidSid);
            }
            8 + usize::from(header[1]) * 4
        };
        if length < 8
            || bytes.len() + length > 65536
            || (relative && u64::from(raw) + length as u64 > 65536)
        {
            return Err(FileError32::InvalidSecurityDescriptor);
        }
        let content = read(memory, component, length)?;
        original_length = if relative {
            original_length.max(raw as usize + length)
        } else {
            original_length + length
        };
        let offset = bytes.len() as u32;
        bytes
            .try_reserve(length)
            .map_err(|_| FileError32::Allocation)?;
        bytes.extend_from_slice(&content);
        bytes[field..field + 4].copy_from_slice(&offset.to_le_bytes());
    }
    Ok((
        SecurityDescriptor32::from_self_relative(&bytes)?,
        original_length as u32,
    ))
}
pub(in crate::process) fn descriptor_parts(
    memory: &AddressSpace,
    information: u32,
    pointers: &[u32],
) -> Result<SecurityDescriptor32, FileError32> {
    let mut header = Vec::new();
    header
        .try_reserve_exact(20)
        .map_err(|_| FileError32::Allocation)?;
    header.resize(20, 0);
    header[0] = 1;
    let control: u16 =
        if information & 4 != 0 { 4 } else { 0 } | if information & 8 != 0 { 16 } else { 0 };
    header[2..4].copy_from_slice(&control.to_le_bytes());
    for (field, flag, pointer) in [
        (4, 1, pointers[0]),
        (8, 2, pointers[1]),
        (16, 4, pointers[2]),
        (12, 8, pointers[3]),
    ] {
        if information & flag != 0 {
            header[field..field + 4].copy_from_slice(&pointer.to_le_bytes());
        }
    }
    components(memory, 0, header).map(|(descriptor, _)| descriptor)
}
pub(in crate::process) fn attributes_descriptor(
    memory: &AddressSpace,
    attributes: u32,
) -> Result<Option<SecurityDescriptor32>, FileError32> {
    if attributes == 0 {
        return Ok(None);
    }
    let bytes = read(memory, attributes, 12)?;
    if u32::from_le_bytes(bytes[..4].try_into().unwrap()) != 12 {
        return Err(FileError32::InvalidParameter);
    }
    let pointer = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    if pointer == 0 {
        Ok(None)
    } else {
        descriptor(memory, pointer).map(Some)
    }
}
