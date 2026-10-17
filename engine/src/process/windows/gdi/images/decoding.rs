use super::types::GdiDibSource32;
use crate::windows::gdi::{BitmapLayout32, GdiError32};

pub(super) struct DibPixels32<'a> {
    pub layout: BitmapLayout32,
    pub colors: Vec<u32>,
    pub pixels: &'a [u8],
}
pub(super) fn dib_pixels(source: GdiDibSource32<'_>) -> Result<DibPixels32<'_>, GdiError32> {
    let header = word(source.info, 0)?;
    let (layout, count, offset, size) = if header == 12 {
        let part = source.info.get(4..12).ok_or(GdiError32::InvalidRequest)?;
        let width = u16::from_le_bytes(part[0..2].try_into().unwrap());
        let height = u16::from_le_bytes(part[2..4].try_into().unwrap());
        if u16::from_le_bytes(part[4..6].try_into().unwrap()) != 1 {
            return Err(GdiError32::InvalidRequest);
        }
        let bits = u16::from_le_bytes(part[6..8].try_into().unwrap());
        let layout = BitmapLayout32::new(i32::from(width), i32::from(height), bits, true)?;
        (
            layout,
            if bits <= 8 { 1usize << bits } else { 0 },
            12usize,
            3usize,
        )
    } else {
        let mut values = [0; 10];
        for (index, value) in values.iter_mut().enumerate() {
            *value = word(source.info, index * 4)?;
        }
        let info = crate::windows::gdi::decode_dib_header(values, || {
            Ok([
                word(source.info, 40)?,
                word(source.info, 44)?,
                word(source.info, 48)?,
            ])
        })?;
        (info.layout, info.colors, info.table_offset as usize, 4usize)
    };
    let end = offset
        .checked_add(count * size)
        .ok_or(GdiError32::InvalidRequest)?;
    let table = source
        .info
        .get(offset..end)
        .ok_or(GdiError32::InvalidRequest)?;
    let mut colors = Vec::new();
    colors
        .try_reserve_exact(count)
        .map_err(|_| GdiError32::Allocation)?;
    colors.extend(
        table
            .chunks_exact(size)
            .map(|part| u32::from(part[2]) | u32::from(part[1]) << 8 | u32::from(part[0]) << 16),
    );
    let pixels = source
        .pixels
        .unwrap_or(source.info.get(end..).ok_or(GdiError32::InvalidRequest)?);
    Ok(DibPixels32 {
        layout,
        colors,
        pixels,
    })
}
fn word(bytes: &[u8], at: usize) -> Result<u32, GdiError32> {
    let bytes = bytes.get(at..at + 4).ok_or(GdiError32::InvalidRequest)?;
    Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
}
impl DibPixels32<'_> {
    pub(super) fn check_pixels(&self) -> Result<(), GdiError32> {
        if self.pixels.len() < self.layout.byte_len()? {
            return Err(GdiError32::InvalidRequest);
        }
        Ok(())
    }
}
