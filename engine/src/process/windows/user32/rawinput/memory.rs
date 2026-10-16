use crate::{
    memory::{Access, AddressSpace, GuestAddress},
    windows::user32::User32Error32 as E,
};
pub(super) fn aligned(pointer: u32) -> Result<(), E> {
    if pointer & 3 != 0 {
        Err(E::Win32(87))
    } else {
        Ok(())
    }
}
pub(super) fn dword(memory: &AddressSpace, pointer: u32) -> Result<u32, E> {
    if pointer == 0 {
        return Err(E::Win32(87));
    }
    let mut bytes = [0; 4];
    memory.read(GuestAddress(pointer), &mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}
pub(super) fn size(memory: &mut AddressSpace, pointer: u32, value: u32) -> Result<(), E> {
    if pointer == 0 {
        return Err(E::Win32(87));
    }
    memory.write(GuestAddress(pointer), &value.to_le_bytes())?;
    Ok(())
}
pub(super) fn vector(length: usize) -> Result<Vec<u8>, E> {
    let mut bytes = Vec::new();
    bytes.try_reserve_exact(length).map_err(|_| E::Allocation)?;
    bytes.resize(length, 0);
    Ok(bytes)
}
pub(super) fn output(memory: &AddressSpace, pointer: u32, length: usize) -> Result<(), E> {
    if pointer == 0 {
        return Err(E::Win32(87));
    }
    aligned(pointer)?;
    memory.check_span(GuestAddress(pointer), length, Access::Write)?;
    Ok(())
}
