use crate::{
    graphics::d3d8::api::HResult,
    memory::{Access, AddressSpace, GuestAddress, WordWrite32},
};

pub(super) fn read_words<const N: usize>(
    memory: &AddressSpace,
    pointer: u32,
) -> Result<[u32; N], HResult> {
    if pointer == 0 || u64::from(pointer) + N as u64 * 4 > 1 << 32 {
        return Err(HResult::INVALID_CALL);
    }
    let mut words = [0; N];
    for (index, word) in words.iter_mut().enumerate() {
        let mut bytes = [0; 4];
        memory.read(GuestAddress(pointer + index as u32 * 4), &mut bytes)?;
        *word = u32::from_le_bytes(bytes);
    }
    Ok(words)
}

pub(super) fn read_bytes(
    memory: &AddressSpace,
    pointer: u32,
    length: usize,
) -> Result<Vec<u8>, HResult> {
    if length == 0 {
        return Ok(Vec::new());
    }
    if pointer == 0 || u64::from(pointer) + length as u64 > 1 << 32 {
        return Err(HResult::INVALID_CALL);
    }
    memory.check_span(GuestAddress(pointer), length, Access::Read)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| HResult::OUT_OF_MEMORY)?;
    bytes.resize(length, 0);
    memory.read(GuestAddress(pointer), &mut bytes)?;
    Ok(bytes)
}

pub(super) fn write_words(
    memory: &mut AddressSpace,
    pointer: u32,
    words: &[u32],
) -> Result<(), HResult> {
    check_output(memory, pointer, words.len() * 4)?;
    let mut bytes = [0; 96 * 16];
    let output = bytes
        .get_mut(..words.len() * 4)
        .ok_or(HResult::INVALID_CALL)?;
    for (word, chunk) in words.iter().zip(output.chunks_exact_mut(4)) {
        chunk.copy_from_slice(&word.to_le_bytes());
    }
    memory.write(GuestAddress(pointer), output)?;
    Ok(())
}

pub(super) fn check_output(
    memory: &AddressSpace,
    pointer: u32,
    length: usize,
) -> Result<(), HResult> {
    if pointer == 0 || u64::from(pointer) + length as u64 > 1 << 32 {
        return Err(HResult::INVALID_CALL);
    }
    memory.check_span(GuestAddress(pointer), length, Access::Write)?;
    Ok(())
}

pub(super) fn word(address: u32, value: u32) -> WordWrite32 {
    WordWrite32 {
        address: GuestAddress(address),
        value,
    }
}

pub(super) fn read_guid(memory: &AddressSpace, pointer: u32) -> Result<[u8; 16], HResult> {
    if pointer == 0 {
        return Err(HResult::INVALID_CALL);
    }
    let mut bytes = [0; 16];
    memory.read(GuestAddress(pointer), &mut bytes)?;
    Ok(bytes)
}
