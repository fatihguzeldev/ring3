use crate::{
    loader::{LoadedModule32, ModuleError32, ModuleSet32},
    memory::{AddressSpace, GuestAddress, PageRange},
};

fn conflict_end(
    memory: &AddressSpace,
    set: &ModuleSet32,
    additions: &[LoadedModule32],
    reserved: &[PageRange],
    base: u32,
    size: u32,
) -> Option<u64> {
    let start = u64::from(base);
    let end = start + u64::from(size);
    for module in set.iter().chain(additions) {
        let module_end = u64::from(module.base()) + u64::from(module.image_size());
        if start < module_end && u64::from(module.base()) < end {
            return Some(module_end);
        }
    }
    for range in reserved {
        let reserved_start = range.first as u64 * 4096;
        let reserved_end = reserved_start + range.count as u64 * 4096;
        if start < reserved_end && reserved_start < end {
            return Some(reserved_end);
        }
    }
    for address in (start..end).step_by(4096) {
        if memory.permissions_at(GuestAddress(address as u32)).is_ok() {
            return Some(address + 4096);
        }
    }
    None
}

pub(in crate::loader::modules) fn place(
    memory: &AddressSpace,
    set: &ModuleSet32,
    additions: &[LoadedModule32],
    reserved: &[PageRange],
    preferred: u32,
    size: u32,
    requested: Option<u32>,
) -> Result<u32, ModuleError32> {
    let valid = |base: u32| {
        base != 0 && base.is_multiple_of(65536) && u64::from(base) + u64::from(size) <= 1 << 32
    };
    if let Some(base) = requested {
        if !valid(base) || conflict_end(memory, set, additions, reserved, base, size).is_some() {
            return Err(ModuleError32::AddressConflict);
        }
        return Ok(base);
    }
    if valid(preferred) && conflict_end(memory, set, additions, reserved, preferred, size).is_none()
    {
        return Ok(preferred);
    }
    let mut candidate = 0x1000_0000_u64;
    while candidate + u64::from(size) <= 1 << 32 {
        let base = candidate as u32;
        match conflict_end(memory, set, additions, reserved, base, size) {
            Some(end) => candidate = end.div_ceil(65536) * 65536,
            None => return Ok(base),
        }
    }
    Err(ModuleError32::AddressConflict)
}
