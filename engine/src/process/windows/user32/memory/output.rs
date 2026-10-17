use super::super::User32CallContext32;
use crate::{
    memory::{AddressSpace, GuestAddress},
    windows::user32::User32Error32,
};

pub(in crate::process::windows::user32) fn encode_text(
    ram: &mut AddressSpace,
    output: u32,
    capacity: usize,
    text: &[u16],
    wide: bool,
    call: User32CallContext32,
) -> Result<u32, User32Error32> {
    let (bytes, count) = encoded_text(capacity, text, wide, call)?;
    if !bytes.is_empty() {
        ram.write(GuestAddress(output), &bytes)?;
    }
    Ok(count)
}

pub(in crate::process::windows::user32) fn encoded_text(
    capacity: usize,
    text: &[u16],
    wide: bool,
    call: User32CallContext32,
) -> Result<(Vec<u8>, u32), User32Error32> {
    if capacity == 0 {
        return Ok((Vec::new(), 0));
    }
    let (bytes, count) = if wide {
        let count = text.len().min(capacity - 1);
        (
            text[..count]
                .iter()
                .copied()
                .chain([0])
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
            count,
        )
    } else {
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(text.len().min(capacity - 1) + 1)
            .map_err(|_| User32Error32::Allocation)?;
        for unit in char::decode_utf16(text.iter().copied()) {
            let value = unit.unwrap_or(char::REPLACEMENT_CHARACTER);
            let mut units = [0; 2];
            let encoded = call
                .ansi
                .encode(value.encode_utf16(&mut units), false)
                .map_err(|_| User32Error32::Win32(1113))?
                .0;
            if bytes.len() + encoded.len() >= capacity {
                break;
            }
            bytes.extend_from_slice(&encoded);
        }
        let count = bytes.len();
        bytes.push(0);
        (bytes, count)
    };
    Ok((bytes, count as u32))
}
