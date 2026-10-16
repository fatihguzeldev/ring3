use super::{
    FileEffect32,
    layout::{Cursor, FORMAT, copied},
};
use crate::input::dinput::{DirectInputResult32 as H, haptics::EffectEnvelope32};

pub fn decode(bytes: &[u8], max_entries: usize, max_bytes: usize) -> Result<Vec<FileEffect32>, H> {
    if bytes.len() > max_bytes {
        return Err(H::OUT_OF_MEMORY);
    }
    let mut cursor = Cursor { bytes, at: 0 };
    if cursor.take(4)? != b"RIFF" {
        return Err(H::INVALID_PARAM);
    }
    let declared = cursor.word()? as usize;
    if declared != 0 && declared.checked_add(8) != Some(bytes.len())
        || cursor.take(4)? != b"FORC"
        || cursor.take(16)? != FORMAT
    {
        return Err(H::INVALID_PARAM);
    }
    let mut result = Vec::new();
    while cursor.at < bytes.len() {
        if result.len() == max_entries {
            return Err(H::OUT_OF_MEMORY);
        }
        if cursor.take(4)? != b"LIST" {
            return Err(H::INVALID_PARAM);
        }
        let length = cursor.word()? as usize;
        let data = cursor.take(length)?;
        cursor.take(length & 1)?;
        let effect = effect(data)?;
        result.try_reserve(1).map_err(|_| H::OUT_OF_MEMORY)?;
        result.push(effect);
    }
    Ok(result)
}
fn effect(bytes: &[u8]) -> Result<FileEffect32, H> {
    let mut cursor = Cursor { bytes, at: 0 };
    if cursor.take(4)? != b"efct" {
        return Err(H::INVALID_PARAM);
    }
    let name = cursor.take(64)?;
    let end = name
        .iter()
        .position(|value| *value == 0)
        .ok_or(H::INVALID_PARAM)?;
    let name = copied(&name[..end])?;
    let mut header = [0u32; 14];
    for word in &mut header {
        *word = cursor.word()?
    }
    if header[0] != 56 || header[7] > 32 || header[7] == 0 {
        return Err(H::INVALID_PARAM);
    }
    let guid = cursor.take(16)?.try_into().unwrap();
    if cursor.word()? != 1 {
        return Err(H::INVALID_PARAM);
    }
    let axis_bytes = cursor.data()?;
    let direction_bytes = cursor.data()?;
    if axis_bytes.len() != header[7] as usize * 4 || direction_bytes.len() != axis_bytes.len() {
        return Err(H::INVALID_PARAM);
    }
    let mut axes = Vec::new();
    let mut direction = Vec::new();
    axes.try_reserve_exact(header[7] as usize)
        .map_err(|_| H::OUT_OF_MEMORY)?;
    direction
        .try_reserve_exact(header[7] as usize)
        .map_err(|_| H::OUT_OF_MEMORY)?;
    axes.extend(
        axis_bytes
            .chunks_exact(4)
            .map(|bytes| u32::from_le_bytes(bytes.try_into().unwrap())),
    );
    direction.extend(
        direction_bytes
            .chunks_exact(4)
            .map(|bytes| i32::from_le_bytes(bytes.try_into().unwrap())),
    );
    let envelope = if header[10] != 0 {
        let data = cursor.data()?;
        if data.len() != 20 {
            return Err(H::INVALID_PARAM);
        }
        let mut envelope = Cursor { bytes: data, at: 0 };
        if envelope.word()? != 20 {
            return Err(H::INVALID_PARAM);
        }
        Some(EffectEnvelope32 {
            attack_level: envelope.word()?,
            attack_time: envelope.word()?,
            fade_level: envelope.word()?,
            fade_time: envelope.word()?,
        })
    } else {
        None
    };
    let type_specific = if header[12] != 0 {
        let data = cursor.data()?;
        if data.len() != header[11] as usize {
            return Err(H::INVALID_PARAM);
        }
        copied(data)?
    } else {
        if header[11] != 0 {
            return Err(H::INVALID_PARAM);
        }
        Vec::new()
    };
    let mut opaque_chunks = Vec::new();
    while cursor.at < bytes.len() {
        if opaque_chunks.len() == 256 {
            return Err(H::OUT_OF_MEMORY);
        }
        let tag = cursor.take(4)?.try_into().unwrap();
        let length = cursor.word()? as usize;
        if length > 131072 {
            return Err(H::OUT_OF_MEMORY);
        }
        let data = copied(cursor.take(length)?)?;
        cursor.take(length & 1)?;
        opaque_chunks.try_reserve(1).map_err(|_| H::OUT_OF_MEMORY)?;
        opaque_chunks.push(super::OpaqueChunk32 { tag, bytes: data });
    }
    let result = FileEffect32 {
        guid,
        name,
        flags: header[1],
        duration: header[2],
        sample_period: header[3],
        gain: header[4],
        trigger: header[5],
        trigger_repeat: header[6],
        axes,
        direction,
        envelope,
        type_specific,
        opaque_chunks,
        start_delay: header[13],
    };
    result.validate()?;
    Ok(result)
}
