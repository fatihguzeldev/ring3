use super::{RegistryError32 as E, RegistryView32};
use crate::windows::text::ordinal_key;
pub(super) const USER_SID: &str = "S-1-5-21-0-0-0-1000";
pub(super) const USER_CLASSES: &str = "S-1-5-21-0-0-0-1000\\Software\\Classes";
pub(super) const MACHINE_CLASSES: &str = "Software\\Classes";
pub(super) const CURRENT_CONFIG: &str = "SYSTEM\\CurrentControlSet\\Hardware Profiles\\Current";
pub(super) fn path(units: &[u16]) -> Result<Vec<u16>, E> {
    if units.len() > 32767 || units.contains(&0) {
        return Err(E::InvalidParameter);
    }
    let mut result = Vec::new();
    result
        .try_reserve_exact(units.len())
        .map_err(|_| E::NotEnoughMemory)?;
    for part in units
        .split(|&unit| unit == 92)
        .filter(|part| !part.is_empty())
    {
        if part.len() > 255 {
            return Err(E::InvalidParameter);
        }
        if !result.is_empty() {
            result.push(92);
        }
        result.extend_from_slice(part);
    }
    Ok(result)
}
pub(super) fn joined(base: &[u16], relative: &[u16]) -> Result<Vec<u16>, E> {
    let relative = path(relative)?;
    if relative.is_empty() {
        return Ok(base.to_vec());
    }
    let count = base
        .len()
        .checked_add(usize::from(!base.is_empty()))
        .and_then(|count| count.checked_add(relative.len()))
        .filter(|&count| count <= 32767)
        .ok_or(E::InvalidParameter)?;
    let mut result = Vec::new();
    result
        .try_reserve_exact(count)
        .map_err(|_| E::NotEnoughMemory)?;
    result.extend_from_slice(base);
    if !base.is_empty() {
        result.push(92);
    }
    result.extend(relative);
    Ok(result)
}
pub(super) fn starts_with(path: &[u16], parent: &[u16]) -> bool {
    path.starts_with(parent) && (path.len() == parent.len() || path.get(parent.len()) == Some(&92))
}
pub(super) fn immediate<'a>(path: &'a [u16], parent: &[u16]) -> Option<&'a [u16]> {
    if !starts_with(&ordinal_key(path), &ordinal_key(parent)) || path.len() <= parent.len() {
        return None;
    }
    let start = parent.len() + usize::from(!parent.is_empty());
    let name = &path[start..];
    (!name.contains(&92)).then_some(name)
}
pub(super) fn view(
    separate: bool,
    flags: u32,
    inherited: RegistryView32,
) -> Result<RegistryView32, E> {
    match flags & 0x300 {
        0 => Ok(if separate {
            inherited
        } else {
            RegistryView32::View32
        }),
        0x100 => Ok(if separate {
            RegistryView32::View64
        } else {
            RegistryView32::View32
        }),
        0x200 => Ok(RegistryView32::View32),
        _ => Err(E::InvalidParameter),
    }
}
pub(super) fn access(raw: u32) -> Result<u32, E> {
    let mut rights = raw & 0x000f_003f;
    if raw & 0x8000_0000 != 0 || raw & 0x2000_0000 != 0 {
        rights |= 0x20019;
    }
    if raw & 0x4000_0000 != 0 {
        rights |= 0x20006;
    }
    if raw & 0x1000_0000 != 0 {
        rights |= 0xf003f;
    }
    if raw & !(0xf000_0000 | 0xf003f | 0x300) != 0 {
        return Err(E::InvalidParameter);
    }
    Ok(rights)
}
