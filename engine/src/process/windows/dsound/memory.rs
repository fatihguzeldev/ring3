use crate::{
    audio::dsound::{HResult, WaveFormat},
    memory::{Access, AddressSpace, GuestAddress, WordWrite32},
};

pub(in crate::process::windows::dsound) fn read<const N: usize>(
    memory: &AddressSpace,
    pointer: u32,
) -> Result<[u8; N], HResult> {
    if pointer == 0 {
        return Err(HResult::INVALID_PARAM);
    }
    let mut bytes = [0; N];
    memory.read(GuestAddress(pointer), &mut bytes)?;
    Ok(bytes)
}
pub(in crate::process::windows::dsound) fn words<const N: usize>(
    memory: &AddressSpace,
    pointer: u32,
) -> Result<[u32; N], HResult> {
    let mut words = [0; N];
    if pointer == 0 || u64::from(pointer) + N as u64 * 4 > 1 << 32 {
        return Err(HResult::INVALID_PARAM);
    }
    for (index, word) in words.iter_mut().enumerate() {
        *word = u32::from_le_bytes(read(memory, pointer + index as u32 * 4)?);
    }
    Ok(words)
}
pub(in crate::process::windows::dsound) fn bytes(
    memory: &AddressSpace,
    pointer: u32,
    length: usize,
) -> Result<Vec<u8>, HResult> {
    if length == 0 {
        return Ok(Vec::new());
    }
    if pointer == 0 {
        return Err(HResult::INVALID_PARAM);
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
pub(in crate::process::windows::dsound) fn output(
    memory: &AddressSpace,
    pointer: u32,
    length: usize,
) -> Result<(), HResult> {
    if pointer == 0 {
        return Err(HResult::INVALID_PARAM);
    }
    memory.check_span(GuestAddress(pointer), length, Access::Write)?;
    if memory.transaction_version() == u64::MAX {
        return Err(HResult::INVALID_PARAM);
    }
    Ok(())
}
pub(in crate::process::windows::dsound) fn write_words(
    memory: &mut AddressSpace,
    pointer: u32,
    words: &[u32],
) -> Result<(), HResult> {
    output(memory, pointer, words.len() * 4)?;
    let mut bytes = [0; 128];
    let target = bytes
        .get_mut(..words.len() * 4)
        .ok_or(HResult::INVALID_PARAM)?;
    for (word, chunk) in words.iter().zip(target.chunks_exact_mut(4)) {
        chunk.copy_from_slice(&word.to_le_bytes());
    }
    memory.write(GuestAddress(pointer), target)?;
    Ok(())
}
pub(in crate::process::windows::dsound) fn word(pointer: u32, value: u32) -> WordWrite32 {
    WordWrite32 {
        address: GuestAddress(pointer),
        value,
    }
}
pub(in crate::process::windows::dsound) fn wave(
    memory: &AddressSpace,
    pointer: u32,
) -> Result<WaveFormat, HResult> {
    let prefix = read::<16>(memory, pointer)?;
    let tag = u16::from_le_bytes(prefix[..2].try_into().unwrap());
    if tag == 0xfffe {
        WaveFormat::parse(&read::<40>(memory, pointer)?)
    } else {
        WaveFormat::parse(&prefix)
    }
}
