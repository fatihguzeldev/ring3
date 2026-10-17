use crate::{
    memory::{AddressSpace, GuestAddress},
    windows::{
        gdi::{GdiError32, LogicalFont32},
        text::CodePage32,
    },
};
pub(super) fn string(
    memory: &AddressSpace,
    pointer: u32,
    wide: bool,
    count: i32,
    cp: CodePage32,
    limit: usize,
) -> Result<Vec<u16>, GdiError32> {
    if count == -1 {
        if pointer == 0 {
            return Err(GdiError32::InvalidRequest);
        }
        let value =
            crate::process::windows::strings::Input32::read(memory, pointer, wide, cp, limit + 1)
                .map_err(|error| match error {
                crate::process::windows::strings::WindowsStringError32::Memory(error) => {
                    GdiError32::Memory(error)
                }
                crate::process::windows::strings::WindowsStringError32::Allocation => {
                    GdiError32::Allocation
                }
                _ => GdiError32::InvalidRequest,
            })?;
        return Ok(value.units);
    }
    if count < 0 || count as usize > limit {
        return Err(GdiError32::InvalidRequest);
    }
    if count == 0 {
        return Ok(Vec::new());
    }
    let length = count as usize * if wide { 2 } else { 1 };
    let mut bytes = super::super::memory::zeroes(length)?;
    memory.read(GuestAddress(pointer), &mut bytes)?;
    if wide {
        Ok(bytes
            .chunks_exact(2)
            .map(|bytes| u16::from_le_bytes(bytes.try_into().unwrap()))
            .collect())
    } else {
        cp.decode(&bytes, false)
            .map_err(|_| GdiError32::InvalidRequest)
    }
}
pub(in crate::process) fn logical_font(
    memory: &AddressSpace,
    pointer: u32,
    wide: bool,
    cp: CodePage32,
) -> Result<LogicalFont32, GdiError32> {
    let mut bytes = super::super::memory::zeroes(if wide { 92 } else { 60 })?;
    memory.read(GuestAddress(pointer), &mut bytes)?;
    let words: [i32; 5] = std::array::from_fn(|index| {
        i32::from_le_bytes(bytes[index * 4..index * 4 + 4].try_into().unwrap())
    });
    let face = if wide {
        bytes[28..]
            .chunks_exact(2)
            .map(|bytes| u16::from_le_bytes(bytes.try_into().unwrap()))
            .take_while(|unit| *unit != 0)
            .collect()
    } else {
        let end = bytes[28..].iter().position(|byte| *byte == 0).unwrap_or(32);
        cp.decode(&bytes[28..28 + end], false)
            .map_err(|_| GdiError32::InvalidRequest)?
    };
    Ok(LogicalFont32 {
        height: words[0],
        width: words[1],
        escapement: words[2],
        orientation: words[3],
        weight: words[4],
        italic: bytes[20] != 0,
        underline: bytes[21] != 0,
        strike_out: bytes[22] != 0,
        charset: bytes[23],
        out_precision: bytes[24],
        clip_precision: bytes[25],
        quality: bytes[26],
        pitch_family: bytes[27],
        face,
    })
}
pub(in crate::process) fn encode_font(
    font: &LogicalFont32,
    wide: bool,
    cp: CodePage32,
) -> Result<Vec<u8>, GdiError32> {
    let mut bytes = super::super::memory::zeroes(if wide { 92 } else { 60 })?;
    for (output, word) in bytes[..20].chunks_exact_mut(4).zip([
        font.height,
        font.width,
        font.escapement,
        font.orientation,
        font.weight,
    ]) {
        output.copy_from_slice(&word.to_le_bytes());
    }
    bytes[20..28].copy_from_slice(&[
        u8::from(font.italic),
        u8::from(font.underline),
        u8::from(font.strike_out),
        font.charset,
        font.out_precision,
        font.clip_precision,
        font.quality,
        font.pitch_family,
    ]);
    let name = encode(&font.face, wide, cp)?;
    let count = name.len().min(bytes.len() - 28 - if wide { 2 } else { 1 });
    bytes[28..28 + count].copy_from_slice(&name[..count]);
    Ok(bytes)
}
pub(super) fn encode(units: &[u16], wide: bool, cp: CodePage32) -> Result<Vec<u8>, GdiError32> {
    if wide {
        Ok(units.iter().flat_map(|unit| unit.to_le_bytes()).collect())
    } else {
        cp.encode_windows(units, 0, b'?')
            .map(|value| value.0)
            .map_err(|_| GdiError32::InvalidRequest)
    }
}
