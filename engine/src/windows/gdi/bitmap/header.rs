use super::super::{BitmapLayout32, GdiError32};

pub(crate) struct DibHeader32 {
    pub layout: BitmapLayout32,
    pub table_offset: u32,
    pub colors: usize,
}
pub(crate) fn decode_dib_header(
    header: [u32; 10],
    masks: impl FnOnce() -> Result<[u32; 3], GdiError32>,
) -> Result<DibHeader32, GdiError32> {
    if header[0] < 40 || header[0] > 124 || header[3] & 0xffff != 1 {
        return Err(GdiError32::InvalidRequest);
    }
    if !matches!(header[4], 0 | 3) {
        return Err(GdiError32::Unsupported);
    }
    let bits = (header[3] >> 16) as u16;
    let mut layout = BitmapLayout32::new(header[1] as i32, header[2] as i32, bits, true)?;
    let mut table_offset = header[0];
    if header[4] == 3 {
        if header[0] != 40 && header[0] < 52 {
            return Err(GdiError32::InvalidRequest);
        }
        layout = layout.with_masks(masks()?)?;
        if header[0] == 40 {
            table_offset += 12;
        }
    }
    let colors = if bits <= 8 && header[8] == 0 {
        1u32 << bits
    } else {
        header[8]
    };
    if colors > 256 || (bits <= 8 && colors > 1u32 << bits) {
        return Err(GdiError32::InvalidRequest);
    }
    Ok(DibHeader32 {
        layout,
        table_offset,
        colors: colors as usize,
    })
}
