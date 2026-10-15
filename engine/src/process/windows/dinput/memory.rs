use crate::{
    input::dinput::DirectInputResult32 as H,
    memory::{Access, AddressSpace, GuestAddress, WordWrite32},
};
pub(super) fn output(memory: &AddressSpace, address: u32, size: usize) -> Result<(), H> {
    if address == 0 || u64::from(address) + size as u64 > 1 << 32 {
        return Err(H::POINTER);
    }
    memory.check_span(GuestAddress(address), size, Access::Write)?;
    Ok(())
}
pub(super) fn read<const N: usize>(memory: &AddressSpace, address: u32) -> Result<[u8; N], H> {
    if address == 0 || u64::from(address) + N as u64 > 1 << 32 {
        return Err(H::POINTER);
    }
    let mut bytes = [0; N];
    memory.read(GuestAddress(address), &mut bytes)?;
    Ok(bytes)
}
pub(super) fn words<const N: usize>(memory: &AddressSpace, address: u32) -> Result<[u32; N], H> {
    let mut values = [0; N];
    for (index, value) in values.iter_mut().enumerate() {
        *value = u32::from_le_bytes(read(
            memory,
            address
                .checked_add(index as u32 * 4)
                .ok_or(H::INVALID_PARAM)?,
        )?);
    }
    Ok(values)
}
pub(super) fn word(address: u32, value: u32) -> WordWrite32 {
    WordWrite32 {
        address: GuestAddress(address),
        value,
    }
}
pub(super) fn bytes(size: usize) -> Result<Vec<u8>, H> {
    let mut result = Vec::new();
    result
        .try_reserve_exact(size)
        .map_err(|_| H::OUT_OF_MEMORY)?;
    result.resize(size, 0);
    Ok(result)
}
pub(super) fn put(output: &mut [u8], at: usize, value: u32) {
    output[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
