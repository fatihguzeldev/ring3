use super::super::memory::{read_bytes, read_words};
use crate::{
    graphics::d3d8::{api::HResult, commands::SignedRect, resources::Rect},
    memory::AddressSpace,
};

pub(super) fn source_rect(memory: &AddressSpace, pointer: u32) -> Result<Option<Rect>, HResult> {
    if pointer == 0 {
        return Ok(None);
    }
    let [left, top, right, bottom] = read_words(memory, pointer)?;
    if [left, top, right, bottom]
        .into_iter()
        .any(|word| word > i32::MAX as u32)
        || left >= right
        || top >= bottom
    {
        return Err(HResult::INVALID_CALL);
    }
    Ok(Some(Rect {
        left,
        top,
        right,
        bottom,
    }))
}

pub(super) fn destination_rect(
    memory: &AddressSpace,
    pointer: u32,
) -> Result<Option<SignedRect>, HResult> {
    if pointer == 0 {
        return Ok(None);
    }
    let [left, top, right, bottom] = read_words(memory, pointer)?.map(|word| word as i32);
    if left >= right || top >= bottom {
        return Err(HResult::INVALID_CALL);
    }
    Ok(Some(SignedRect {
        left,
        top,
        right,
        bottom,
    }))
}

pub(super) fn dirty_region(
    memory: &AddressSpace,
    pointer: u32,
    maximum: u32,
) -> Result<Vec<Rect>, HResult> {
    if pointer == 0 {
        return Ok(Vec::new());
    }
    let [size, kind, count, region_bytes, left, top, right, bottom] = read_words(memory, pointer)?;
    let bytes = count
        .checked_mul(16)
        .filter(|length| *length <= maximum)
        .ok_or(HResult::INVALID_CALL)?;
    if size != 32
        || kind != 1
        || (region_bytes != 0 && region_bytes != bytes)
        || left > right
        || top > bottom
    {
        return Err(HResult::INVALID_CALL);
    }
    let buffer = pointer.checked_add(32).ok_or(HResult::INVALID_CALL)?;
    let data = read_bytes(memory, buffer, bytes as usize)?;
    let mut rectangles = Vec::new();
    rectangles
        .try_reserve_exact(count as usize)
        .map_err(|_| HResult::OUT_OF_MEMORY)?;
    for data in data.chunks_exact(16) {
        let values: [u32; 4] = std::array::from_fn(|index| {
            u32::from_le_bytes(data[index * 4..index * 4 + 4].try_into().unwrap())
        });
        let [x, y, x2, y2] = values;
        if values.into_iter().any(|value| value > i32::MAX as u32)
            || x >= x2
            || y >= y2
            || x < left
            || y < top
            || x2 > right
            || y2 > bottom
        {
            return Err(HResult::INVALID_CALL);
        }
        rectangles.push(Rect {
            left: x,
            top: y,
            right: x2,
            bottom: y2,
        });
    }
    Ok(rectangles)
}
