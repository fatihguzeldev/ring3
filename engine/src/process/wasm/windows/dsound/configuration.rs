use crate::{
    memory::{GuestAddress, PageRange},
    process::{
        DSoundConfig32, HostError,
        wasm::{mutate, windows::input},
    },
};

pub(crate) fn configure_dsound(length: u32) -> u32 {
    mutate(|instance| {
        let config = decode(input(instance, length)?)?;
        instance.configure_dsound(config)
    })
}

pub(crate) fn register_dsound_module(base: u32) -> u32 {
    mutate(|instance| instance.register_dsound_module(base))
}

fn decode(bytes: &[u8]) -> Result<DSoundConfig32, HostError> {
    if bytes.len() != 64
        || &bytes[..4] != b"R3CS"
        || u16::from_le_bytes(bytes[4..6].try_into().unwrap()) != 1
        || u16::from_le_bytes(bytes[6..8].try_into().unwrap()) != 1
    {
        return Err(HostError::InvalidRequest);
    }
    let words: [u32; 16] = std::array::from_fn(|index| {
        u32::from_le_bytes(bytes[index * 4..index * 4 + 4].try_into().unwrap())
    });
    if words[2] != 64
        || words[3] != 0
        || words[12] > 1
        || words[13..].iter().any(|value| *value != 0)
    {
        return Err(HostError::InvalidRequest);
    }
    Ok(DSoundConfig32 {
        objects: PageRange::new(GuestAddress(words[4]), words[5]).map_err(HostError::Memory)?,
        locks: PageRange::new(GuestAddress(words[6]), words[7]).map_err(HostError::Memory)?,
        max_objects: words[8],
        max_locks: words[9],
        max_notifications: words[10],
        max_capture_bytes: words[11],
        capture_available: words[12] != 0,
    })
}
