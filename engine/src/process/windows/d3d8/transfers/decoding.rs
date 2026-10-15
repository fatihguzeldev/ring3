use super::super::memory::read_bytes;
use crate::{
    graphics::d3d8::{api::HResult, resources::Rect},
    memory::AddressSpace,
};

type CopyRegions = (Vec<Rect>, Option<Vec<(u32, u32)>>);

pub(super) fn copy_regions(
    memory: &AddressSpace,
    rectangles: u32,
    points: u32,
    count: u32,
    maximum: u32,
) -> Result<CopyRegions, HResult> {
    if count == 0 {
        return Ok((Vec::new(), None));
    }
    let length = count
        .checked_mul(16)
        .filter(|length| *length <= maximum)
        .ok_or(HResult::INVALID_CALL)?;
    let bytes = read_bytes(memory, rectangles, length as usize)?;
    let mut copied = Vec::new();
    copied
        .try_reserve_exact(count as usize)
        .map_err(|_| HResult::OUT_OF_MEMORY)?;
    for bytes in bytes.chunks_exact(16) {
        let [left, top, right, bottom] = std::array::from_fn(|index| {
            u32::from_le_bytes(bytes[index * 4..index * 4 + 4].try_into().unwrap())
        });
        if [left, top, right, bottom]
            .into_iter()
            .any(|value| value > i32::MAX as u32)
            || left >= right
            || top >= bottom
        {
            return Err(HResult::INVALID_CALL);
        }
        copied.push(Rect {
            left,
            top,
            right,
            bottom,
        });
    }
    let destinations = if points == 0 {
        None
    } else {
        let bytes = read_bytes(memory, points, count as usize * 8)?;
        let mut destinations = Vec::new();
        destinations
            .try_reserve_exact(count as usize)
            .map_err(|_| HResult::OUT_OF_MEMORY)?;
        for bytes in bytes.chunks_exact(8) {
            let x = u32::from_le_bytes(bytes[..4].try_into().unwrap());
            let y = u32::from_le_bytes(bytes[4..].try_into().unwrap());
            if x > i32::MAX as u32 || y > i32::MAX as u32 {
                return Err(HResult::INVALID_CALL);
            }
            destinations.push((x, y));
        }
        Some(destinations)
    };
    Ok((copied, destinations))
}
