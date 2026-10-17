use super::super::{context::GdiRuntime32, memory};
use crate::{
    memory::AddressSpace,
    windows::gdi::{Brush32, ExtendedPen32, GdiError32, GdiObject32, Pen32},
};

impl GdiRuntime32 {
    pub(in crate::process::windows::gdi) fn ext_create_pen(
        &mut self,
        memory: &AddressSpace,
        args: &[u32],
    ) -> Result<u32, GdiError32> {
        let brush = memory::words(memory, args[2])?;
        self.ext_create_pen_with_brush(memory, args, brush, None)
    }
    pub(in crate::process::windows::gdi) fn ext_create_pen_with_brush(
        &mut self,
        memory: &AddressSpace,
        args: &[u32],
        brush: [u32; 3],
        limit: Option<u32>,
    ) -> Result<u32, GdiError32> {
        let style = args[0];
        let geometric = style & 0xf0000 == 0x10000;
        let kind = style & 15;
        if style & !0x0001330f != 0
            || style & 0xf0000 > 0x10000
            || kind > 8
            || style & 0xf00 > 0x200
            || style & 0xf000 > 0x2000
            || args[1] > i32::MAX as u32
            || (!geometric && args[1] != 1)
            || (!geometric && style & 0xff00 != 0)
            || (!geometric && kind == 6)
            || (geometric && kind == 8)
            || args[3] > 16
            || (kind != 7 && (args[3] != 0 || args[4] != 0))
            || (kind == 7 && args[3] == 0)
        {
            return Err(GdiError32::InvalidRequest);
        }
        let [brush_style, color, hatch] = brush;
        if !geometric && brush_style != 0 {
            return Err(GdiError32::InvalidRequest);
        }
        let brush = match brush_style {
            0 => Brush32::Solid(color & 0xffffff),
            1 => Brush32::Null,
            2 if hatch <= 5 => Brush32::Hatch {
                style: hatch,
                color: color & 0xffffff,
            },
            2 => return Err(GdiError32::InvalidRequest),
            3 => {
                let GdiObject32::Bitmap(layout) = self.model.object(hatch)? else {
                    return Err(GdiError32::InvalidHandle);
                };
                if layout.dib {
                    return Err(GdiError32::InvalidRequest);
                }
                Brush32::Pattern {
                    bitmap: hatch,
                    monochrome: layout.bits_per_pixel == 1,
                }
            }
            6 if color & 0xffff <= 1 => Brush32::DibPattern {
                address: hatch,
                usage: color & 0xffff,
            },
            6 => return Err(GdiError32::InvalidRequest),
            _ => return Err(GdiError32::Unsupported),
        };
        let mut entries = [0; 16];
        if kind == 7 {
            let input: [u32; 16] = read_entries(memory, args[4], args[3] as usize)?;
            entries = input;
            if entries[..args[3] as usize].iter().all(|&value| value == 0) {
                return Err(GdiError32::InvalidRequest);
            }
        }
        let pen = Pen32 {
            style,
            width: args[1].max(1) as i32,
            color: color & 0xffffff,
            extended: Some(ExtendedPen32 {
                brush,
                entries,
                count: args[3],
            }),
        };
        let prepared = self.model.prepare_object(GdiObject32::Pen(pen))?;
        let pattern = match brush {
            Brush32::Pattern { bitmap, .. } => {
                Some(self.prepare_pattern_surface(memory, bitmap, prepared.handle())?)
            }
            Brush32::DibPattern { address, usage } => Some(self.prepare_dib_pattern_surface(
                memory,
                address,
                usage,
                prepared.handle(),
                limit,
            )?),
            _ => None,
        };
        let handle = self.model.commit_new(prepared);
        if let Some(pattern) = pattern {
            self.publish_surface(pattern);
        }
        Ok(handle)
    }
}
fn read_entries(
    memory: &AddressSpace,
    address: u32,
    count: usize,
) -> Result<[u32; 16], GdiError32> {
    let mut bytes = [0; 64];
    memory.read(
        crate::memory::GuestAddress(address),
        &mut bytes[..count * 4],
    )?;
    Ok(core::array::from_fn(|index| {
        u32::from_le_bytes(bytes[index * 4..index * 4 + 4].try_into().unwrap())
    }))
}
