use crate::{
    files::FileError32,
    memory::{Access, AddressSpace, GuestAddress},
};

pub(in crate::process::windows::files) fn output(
    memory: &AddressSpace,
    address: u32,
    length: usize,
) -> Result<(), FileError32> {
    if length == 0 {
        return Ok(());
    }
    if address == 0 || u64::from(address) + length as u64 > 1 << 32 {
        return Err(FileError32::NoAccess);
    }
    memory
        .check_span(GuestAddress(address), length, Access::Write)
        .map_err(|_| FileError32::NoAccess)
}
pub(in crate::process::windows::files) fn read<const N: usize>(
    memory: &AddressSpace,
    address: u32,
) -> Result<[u8; N], FileError32> {
    if address == 0 || u64::from(address) + N as u64 > 1 << 32 {
        return Err(FileError32::NoAccess);
    }
    let mut bytes = [0; N];
    memory
        .read(GuestAddress(address), &mut bytes)
        .map_err(|_| FileError32::NoAccess)?;
    Ok(bytes)
}
pub(in crate::process::windows::files) fn dword(
    memory: &AddressSpace,
    address: u32,
) -> Result<u32, FileError32> {
    Ok(u32::from_le_bytes(read(memory, address)?))
}
pub(in crate::process::windows::files) fn path(
    memory: &AddressSpace,
    address: u32,
    wide: bool,
    cp: crate::windows::text::CodePage32,
) -> Result<Vec<u16>, FileError32> {
    if address == 0 {
        return Err(FileError32::InvalidParameter);
    }
    let mut units = Vec::new();
    let mut bytes = Vec::new();
    let maximum = 32767usize;
    let raw_maximum = maximum
        * if wide {
            1
        } else {
            cp.maximum_char_size() as usize
        };
    for index in 0..=raw_maximum {
        let at = address
            .checked_add(index as u32 * if wide { 2 } else { 1 })
            .ok_or(FileError32::NoAccess)?;
        if wide {
            let unit = u16::from_le_bytes(read(memory, at)?);
            if unit == 0 {
                return if units.len() <= maximum {
                    Ok(units)
                } else {
                    Err(FileError32::InvalidName)
                };
            }
            units.try_reserve(1).map_err(|_| FileError32::Allocation)?;
            units.push(unit);
        } else {
            let byte = read::<1>(memory, at)?[0];
            if byte == 0 {
                let units = cp
                    .decode(&bytes, false)
                    .map_err(|_| FileError32::InvalidName)?;
                return if units.len() <= maximum {
                    Ok(units)
                } else {
                    Err(FileError32::InvalidName)
                };
            }
            bytes.try_reserve(1).map_err(|_| FileError32::Allocation)?;
            bytes.push(byte);
        }
    }
    Err(FileError32::InvalidName)
}
pub(in crate::process::windows::files) fn encoded(
    units: &[u16],
    wide: bool,
    nul: bool,
    cp: crate::windows::text::CodePage32,
) -> Result<Vec<u8>, FileError32> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact((units.len() + usize::from(nul)) * if wide { 2 } else { 1 })
        .map_err(|_| FileError32::Allocation)?;
    if wide {
        for &unit in units {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
    } else {
        bytes.extend(
            cp.encode(units, false)
                .map_err(|_| FileError32::InvalidName)?
                .0,
        );
    }
    if nul {
        bytes.extend_from_slice(if wide { &[0, 0] } else { &[0] });
    }
    Ok(bytes)
}
pub(in crate::process::windows::files) fn inherit(
    memory: &AddressSpace,
    address: u32,
) -> Result<bool, FileError32> {
    if address == 0 {
        return Ok(false);
    }
    let bytes = read::<12>(memory, address)?;
    if u32::from_le_bytes(bytes[..4].try_into().unwrap()) != 12 {
        return Err(FileError32::InvalidParameter);
    }
    Ok(u32::from_le_bytes(bytes[8..].try_into().unwrap()) != 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{PageRange, Permissions};
    #[test]
    fn ansi_uses_cp1252_and_wide_paths_preserve_surrogate_units() {
        let mut memory = AddressSpace::new(2).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                Permissions::READ_WRITE,
            )
            .unwrap();
        memory
            .write(GuestAddress(0x1000), &[b'C', b':', b'\\', 0x80, 0x9c, 0])
            .unwrap();
        assert_eq!(
            path(&memory, 0x1000, false, Default::default()).unwrap(),
            [67, 58, 92, 0x20ac, 0x153]
        );
        assert_eq!(
            encoded(&[0x20ac, 0x153], false, true, Default::default()).unwrap(),
            [0x80, 0x9c, 0]
        );
        memory
            .write(GuestAddress(0x1000), &[0, 0xd8, 0, 0])
            .unwrap();
        assert_eq!(
            path(&memory, 0x1000, true, Default::default()).unwrap(),
            [0xd800]
        );
        assert_eq!(
            encoded(&[0xd800], true, true, Default::default()).unwrap(),
            [0, 0xd8, 0, 0]
        );
    }
}
