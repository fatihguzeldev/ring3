use super::super::memory::read_bytes;
use crate::{
    graphics::d3d8::{api::HResult, resources::Rect, state::Viewport},
    memory::AddressSpace,
};

pub(super) fn vertex_bytes(
    memory: &AddressSpace,
    pointer: u32,
    first: u32,
    count: u32,
    stride: u32,
    maximum: u32,
) -> Result<Vec<u8>, HResult> {
    if stride == 0 || stride > 2048 || first.checked_add(count).is_none() {
        return Err(HResult::INVALID_CALL);
    }
    let offset = first.checked_mul(stride).ok_or(HResult::INVALID_CALL)?;
    let length = count
        .checked_mul(stride)
        .filter(|length| *length <= maximum)
        .ok_or(HResult::INVALID_CALL)?;
    let pointer = pointer.checked_add(offset).ok_or(HResult::INVALID_CALL)?;
    read_bytes(memory, pointer, length as usize)
}

pub(super) fn clear_rectangles(
    memory: &AddressSpace,
    pointer: u32,
    count: u32,
    viewport: Viewport,
    maximum: u32,
) -> Result<Vec<Rect>, HResult> {
    let length = count
        .checked_mul(16)
        .filter(|length| *length <= maximum)
        .ok_or(HResult::INVALID_CALL)?;
    let bytes = read_bytes(memory, pointer, length as usize)?;
    let mut rectangles = Vec::new();
    rectangles
        .try_reserve_exact(count as usize)
        .map_err(|_| HResult::OUT_OF_MEMORY)?;
    for bytes in bytes.chunks_exact(16) {
        let values: [i32; 4] = std::array::from_fn(|index| {
            i32::from_le_bytes(bytes[index * 4..index * 4 + 4].try_into().unwrap())
        });
        let [left, top, right, bottom] = values;
        if left > right || top > bottom {
            return Err(HResult::INVALID_CALL);
        }
        let left = i64::from(left).max(i64::from(viewport.x));
        let top = i64::from(top).max(i64::from(viewport.y));
        let right = i64::from(right).min(i64::from(viewport.x) + i64::from(viewport.width));
        let bottom = i64::from(bottom).min(i64::from(viewport.y) + i64::from(viewport.height));
        if left < right && top < bottom {
            rectangles.push(Rect {
                left: left as u32,
                top: top as u32,
                right: right as u32,
                bottom: bottom as u32,
            });
        }
    }
    Ok(rectangles)
}
