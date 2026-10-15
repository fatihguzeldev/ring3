use crate::{
    files::FileError32,
    memory::{Access, AddressSpace, GuestAddress},
};

const CP1252: [u16; 32] = [
    0x20ac, 0x81, 0x201a, 0x192, 0x201e, 0x2026, 0x2020, 0x2021, 0x2c6, 0x2030, 0x160, 0x2039,
    0x152, 0x8d, 0x17d, 0x8f, 0x90, 0x2018, 0x2019, 0x201c, 0x201d, 0x2022, 0x2013, 0x2014, 0x2dc,
    0x2122, 0x161, 0x203a, 0x153, 0x9d, 0x17e, 0x178,
];

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
) -> Result<Vec<u16>, FileError32> {
    if address == 0 {
        return Err(FileError32::InvalidParameter);
    }
    let mut units = Vec::new();
    units
        .try_reserve(260)
        .map_err(|_| FileError32::Allocation)?;
    for index in 0..32_768u32 {
        let at = address
            .checked_add(index * if wide { 2 } else { 1 })
            .ok_or(FileError32::NoAccess)?;
        let unit = if wide {
            u16::from_le_bytes(read(memory, at)?)
        } else {
            let byte = read::<1>(memory, at)?[0];
            if (0x80..0xa0).contains(&byte) {
                CP1252[usize::from(byte - 0x80)]
            } else {
                u16::from(byte)
            }
        };
        if unit == 0 {
            return Ok(units);
        }
        units.try_reserve(1).map_err(|_| FileError32::Allocation)?;
        units.push(unit);
    }
    Err(FileError32::InvalidName)
}
pub(super) fn encoded(units: &[u16], wide: bool, nul: bool) -> Result<Vec<u8>, FileError32> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact((units.len() + usize::from(nul)) * if wide { 2 } else { 1 })
        .map_err(|_| FileError32::Allocation)?;
    if wide {
        for &unit in units {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
    } else {
        for unit in char::decode_utf16(units.iter().copied()) {
            let code = unit.map_or(u32::from(b'?'), |unit| unit as u32);
            bytes.push(if code < 0x80 || (0xa0..=0xff).contains(&code) {
                code as u8
            } else {
                CP1252
                    .iter()
                    .position(|&value| u32::from(value) == code)
                    .map_or(b'?', |index| 0x80 + index as u8)
            });
        }
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
            path(&memory, 0x1000, false).unwrap(),
            [67, 58, 92, 0x20ac, 0x153]
        );
        assert_eq!(
            encoded(&[0x20ac, 0x153], false, true).unwrap(),
            [0x80, 0x9c, 0]
        );
        memory
            .write(GuestAddress(0x1000), &[0, 0xd8, 0, 0])
            .unwrap();
        assert_eq!(path(&memory, 0x1000, true).unwrap(), [0xd800]);
        assert_eq!(encoded(&[0xd800], true, true).unwrap(), [0, 0xd8, 0, 0]);
    }
}
