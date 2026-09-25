use super::{Access, Cpu32, DispatchError, GuestMemory, MemoryError, Register32, guest};

pub(super) const ENTRY: u32 = super::initializers::BASE + 0x100;
const MAX_BYTES: u32 = 16 * 1024 * 1024;
// four saved registers, sixteen local bytes, internal call and cdecl comparator frame.
const WORKSPACE: u32 = 48;

pub(super) fn start(
    cpu: &mut Cpu32,
    memory: &GuestMemory,
    args: &[u32],
) -> Result<bool, DispatchError> {
    let (base, count, width, compare) = (args[0], args[1], args[2], args[3]);
    if width == 0 || compare == 0 || (base == 0 && count != 0) {
        return Err(DispatchError::Unsupported);
    }
    if count < 2 {
        cpu.set_register(Register32::Eax, 0);
        return Ok(false);
    }
    let length = count
        .checked_mul(width)
        .ok_or(MemoryError::AddressOverflow)?;
    if length > MAX_BYTES {
        return Err(DispatchError::Unsupported);
    }
    let stack = cpu.register(Register32::Esp);
    let low = stack
        .checked_sub(WORKSPACE)
        .ok_or(MemoryError::AddressOverflow)?;
    let high = u64::from(stack) + 20;
    if u64::from(base) < high && u64::from(base) + u64::from(length) > u64::from(low) {
        return Err(DispatchError::Unsupported);
    }
    let length = usize::try_from(length).expect("bounded array bytes");
    guest::check(memory, base, length, Access::Read)?;
    guest::check(memory, base, length, Access::Write)?;
    guest::check(memory, compare, 1, Access::Execute)?;
    guest::check(memory, low, WORKSPACE as usize, Access::Read)?;
    guest::check(memory, low, WORKSPACE as usize, Access::Write)?;
    cpu.eip = ENTRY;
    Ok(true)
}
