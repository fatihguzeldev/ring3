use super::{FileEffect32, layout::FORMAT};
use crate::input::dinput::DirectInputResult32 as H;

pub fn encode(effects: &[FileEffect32], max_bytes: usize) -> Result<Vec<u8>, H> {
    let mut length = 28usize;
    for effect in effects {
        effect.validate()?;
        let body = 4usize
            + 64
            + 56
            + 16
            + 4
            + 16
            + effect.axes.len() * 8
            + usize::from(effect.envelope.is_some()) * 28
            + if effect.type_specific.is_empty() {
                0
            } else {
                8 + effect.type_specific.len() + (effect.type_specific.len() & 1)
            }
            + effect
                .opaque_chunks
                .iter()
                .map(|chunk| 8 + chunk.bytes.len() + (chunk.bytes.len() & 1))
                .sum::<usize>();
        length = length
            .checked_add(8 + body + (body & 1))
            .ok_or(H::OUT_OF_MEMORY)?;
    }
    if length > max_bytes || length > u32::MAX as usize {
        return Err(H::OUT_OF_MEMORY);
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| H::OUT_OF_MEMORY)?;
    bytes.extend_from_slice(b"RIFF\0\0\0\0FORC");
    bytes.extend_from_slice(&FORMAT);
    for effect in effects {
        bytes.extend_from_slice(b"LIST");
        let size_at = bytes.len();
        word(&mut bytes, 0);
        let start = bytes.len();
        bytes.extend_from_slice(b"efct");
        let name_len = effect.name.len().min(63);
        bytes.extend_from_slice(&effect.name[..name_len]);
        bytes.resize(bytes.len() + 64 - name_len, 0);
        for value in [
            56,
            effect.flags,
            effect.duration,
            effect.sample_period,
            effect.gain,
            effect.trigger,
            effect.trigger_repeat,
            effect.axes.len() as u32,
            0x20,
            0x40,
            if effect.envelope.is_some() { 0x80 } else { 0 },
            effect.type_specific.len() as u32,
            if effect.type_specific.is_empty() {
                0
            } else {
                0x100
            },
            effect.start_delay,
        ] {
            word(&mut bytes, value)
        }
        bytes.extend_from_slice(&effect.guid);
        word(&mut bytes, 1);
        bytes.extend_from_slice(b"data");
        word(&mut bytes, effect.axes.len() as u32 * 4);
        for value in &effect.axes {
            word(&mut bytes, *value)
        }
        bytes.extend_from_slice(b"data");
        word(&mut bytes, effect.direction.len() as u32 * 4);
        for value in &effect.direction {
            word(&mut bytes, *value as u32)
        }
        if let Some(envelope) = effect.envelope {
            bytes.extend_from_slice(b"data");
            word(&mut bytes, 20);
            for value in [
                20,
                envelope.attack_level,
                envelope.attack_time,
                envelope.fade_level,
                envelope.fade_time,
            ] {
                word(&mut bytes, value)
            }
        }
        if !effect.type_specific.is_empty() {
            data(&mut bytes, &effect.type_specific)
        }
        for chunk in &effect.opaque_chunks {
            bytes.extend_from_slice(&chunk.tag);
            word(&mut bytes, chunk.bytes.len() as u32);
            bytes.extend_from_slice(&chunk.bytes);
            if chunk.bytes.len() & 1 != 0 {
                bytes.push(0);
            }
        }
        let length = bytes.len() - start;
        bytes[size_at..size_at + 4].copy_from_slice(&(length as u32).to_le_bytes());
        if length & 1 != 0 {
            bytes.push(0)
        }
    }
    assert_eq!(bytes.len(), length);
    Ok(bytes)
}
fn word(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes())
}
fn data(bytes: &mut Vec<u8>, value: &[u8]) {
    bytes.extend_from_slice(b"data");
    word(bytes, value.len() as u32);
    bytes.extend_from_slice(value);
    if value.len() & 1 != 0 {
        bytes.push(0)
    }
}
