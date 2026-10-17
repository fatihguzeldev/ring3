use super::{context::GdiRuntime32, memory, storage::SurfaceKey32};
use crate::{
    memory::{AddressSpace, GuestAddress},
    windows::gdi::{Brush32, GdiApi32, GdiError32, GdiObject32, Pen32},
};
impl GdiRuntime32 {
    pub(super) fn create_object(
        &mut self,
        memory: &AddressSpace,
        api: GdiApi32,
        args: &[u32],
    ) -> Result<u32, GdiError32> {
        if api == GdiApi32::CreatePatternBrush {
            return self.create_pattern_brush(memory, args[0]);
        }
        let object = match api {
            GdiApi32::CreateSolidBrush => GdiObject32::Brush(Brush32::Solid(args[0] & 0x00ff_ffff)),
            GdiApi32::CreateHatchBrush if args[0] <= 5 => GdiObject32::Brush(Brush32::Hatch {
                style: args[0],
                color: args[1] & 0x00ff_ffff,
            }),
            GdiApi32::CreateHatchBrush => return Err(GdiError32::InvalidRequest),
            GdiApi32::CreateBrushIndirect => {
                let [style, color, hatch] = memory::words(memory, args[0])?;
                if style == 3 {
                    return self.create_pattern_brush(memory, hatch);
                }
                GdiObject32::Brush(match style {
                    0 => Brush32::Solid(color & 0x00ff_ffff),
                    1 => Brush32::Null,
                    2 if hatch <= 5 => Brush32::Hatch {
                        style: hatch,
                        color: color & 0x00ff_ffff,
                    },
                    _ => return Err(GdiError32::Unsupported),
                })
            }
            GdiApi32::CreatePen | GdiApi32::CreatePenIndirect => {
                let (style, width, color) = if api == GdiApi32::CreatePen {
                    (args[0], args[1] as i32, args[2])
                } else {
                    let [style, width, _, color] = memory::words(memory, args[0])?;
                    (style, width as i32, color)
                };
                if style > 6 || width == i32::MIN {
                    return Err(GdiError32::InvalidRequest);
                }
                GdiObject32::Pen(Pen32 {
                    style,
                    width: width.abs().max(1),
                    color: color & 0x00ff_ffff,
                })
            }
            _ => return Err(GdiError32::InvalidRequest),
        };
        let prepared = self.model.prepare_object(object)?;
        Ok(self.model.commit_new(prepared))
    }
    pub(super) fn delete_object(
        &mut self,
        memory: &mut AddressSpace,
        handle: u32,
    ) -> Result<u32, GdiError32> {
        if self.model.dc_kind(handle).is_ok() {
            self.model.delete_dc(handle)?;
            self.prune_defaults();
            return Ok(1);
        }
        self.model.check_delete_object(handle)?;
        if let Ok(index) = self.surface_index(SurfaceKey32::Bitmap(handle)) {
            if self.surfaces[index].storage.section().is_some() {
                return Err(GdiError32::InvalidRequest);
            }
            self.surfaces[index].storage.check(memory)?;
            if let Some(range) = self.surfaces[index].storage.range() {
                memory.unmap(range)?;
            }
            let surface = self.surfaces.remove(index);
            self.bytes -= surface.layout.byte_len()?;
        }
        if let Ok(index) = self.surface_index(SurfaceKey32::PatternBrush(handle)) {
            let surface = self.surfaces.remove(index);
            self.bytes -= surface.layout.byte_len()?;
        }
        self.model.delete_object(handle)?;
        Ok(1)
    }
    pub(super) fn get_object(
        &self,
        memory: &mut AddressSpace,
        args: &[u32],
    ) -> Result<u32, GdiError32> {
        if let Ok(entries) = self.model.palette_entries(args[0]) {
            if args[2] == 0 {
                return Ok(2);
            }
            let bytes = (entries.len() as u16).to_le_bytes();
            let count = (args[1] as usize).min(bytes.len());
            memory.write(GuestAddress(args[2]), &bytes[..count])?;
            return Ok(count as u32);
        }
        let object = self.model.object(args[0])?;
        let words: Vec<u32> = match object {
            GdiObject32::Pen(pen) => vec![pen.style, pen.width as u32, 0, pen.color],
            GdiObject32::Brush(brush) => match brush {
                Brush32::System(index) => vec![0, self.system_color(index), 0],
                Brush32::Solid(color) => vec![0, color, 0],
                Brush32::Null => vec![1, 0, 0],
                Brush32::Pattern { bitmap, .. } => vec![3, 0, bitmap],
                Brush32::Hatch { style, color } => vec![2, color, style],
            },
            GdiObject32::Bitmap(layout) => {
                let mut words = vec![
                    0,
                    layout.width,
                    layout.height,
                    layout.stride,
                    1 | u32::from(layout.bits_per_pixel) << 16,
                    0,
                ];
                if let Ok(index) = self.surface_index(SurfaceKey32::Bitmap(args[0]))
                    && let Some(address) = self.surfaces[index].storage.address()
                {
                    words[5] = address;
                    if args[1] >= 84 {
                        let compression = if layout.bits_per_pixel == 16 {
                            3
                        } else {
                            layout.compression()
                        };
                        let masks = layout.masks.unwrap_or(if layout.bits_per_pixel == 16 {
                            [0x7c00, 0x03e0, 0x001f]
                        } else {
                            [0; 3]
                        });
                        words.extend([
                            40,
                            layout.width,
                            if layout.top_down {
                                0u32.wrapping_sub(layout.height)
                            } else {
                                layout.height
                            },
                            1 | u32::from(layout.bits_per_pixel) << 16,
                            compression,
                            layout.byte_len()? as u32,
                            0,
                            0,
                            self.surfaces[index].palette.len() as u32,
                            0,
                            0,
                            0,
                            0,
                            0,
                            0,
                        ]);
                        words[16..19].copy_from_slice(&masks);
                        if let Some(section) = self.surfaces[index].storage.section() {
                            words[19] = section.section;
                            words[20] = section.offset;
                        }
                    }
                }
                words
            }
            _ => return Err(GdiError32::Unsupported),
        };
        if args[2] == 0 {
            return Ok(words.len() as u32 * 4);
        }
        let mut bytes = memory::zeroes(words.len() * 4)?;
        for (part, word) in bytes.chunks_exact_mut(4).zip(words) {
            part.copy_from_slice(&word.to_le_bytes());
        }
        let count = bytes.len().min(args[1] as usize);
        memory.write(GuestAddress(args[2]), &bytes[..count])?;
        Ok(count as u32)
    }
}
