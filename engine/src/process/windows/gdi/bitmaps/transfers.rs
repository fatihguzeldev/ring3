use super::super::{
    bitmaps::{bounds, color_to_quad, dib_info},
    context::GdiRuntime32,
    memory,
    raster::{BlitImage32, BlitRequest32},
    storage::SurfaceKey32,
};
use crate::{
    memory::{AddressSpace, GuestAddress},
    windows::{
        gdi::{BitmapLayout32, GdiApi32, GdiError32},
        user32::Point32,
    },
};
impl GdiRuntime32 {
    pub(in crate::process::windows::gdi) fn dib_transfer(
        &mut self,
        memory: &mut AddressSpace,
        api: GdiApi32,
        args: &[u32],
    ) -> Result<u32, GdiError32> {
        if args[0] != 0 {
            self.model.dc_kind(args[0])?;
        }
        self.model.check_delete_object(args[1])?;
        let index = self.surface_index(SurfaceKey32::Bitmap(args[1]))?;
        let source_layout = self.surfaces[index].layout;
        let mut header: [u32; 10] = memory::words(memory, args[5])?;
        if header[0] < 40 || header[0] > 124 || args[6] != 0 {
            return Err(GdiError32::InvalidRequest);
        }
        if api == GdiApi32::GetDIBits && (args[4] == 0 || header[3] >> 16 == 0) {
            let header_only = header[3] >> 16 == 0;
            header[1] = source_layout.width;
            header[2] = source_layout.height;
            header[3] = 1 | u32::from(source_layout.bits_per_pixel) << 16;
            header[4] = source_layout.compression();
            header[5] = BitmapLayout32::new(
                source_layout.width as i32,
                source_layout.height as i32,
                source_layout.bits_per_pixel,
                true,
            )?
            .byte_len()? as u32;
            header[8] = self.surfaces[index].palette.len() as u32;
            if header_only {
                memory::write_words(memory, args[5], &header)?;
            } else {
                let info = dib_description(
                    memory,
                    args[5],
                    header,
                    source_layout,
                    &self.surfaces[index].palette,
                )?;
                memory.write(GuestAddress(args[5]), &info)?;
            }
            return Ok(source_layout.height);
        }
        let (layout, colors) = dib_info(memory, args[5], args[6])?;
        if layout.width != source_layout.width {
            return Err(GdiError32::InvalidRequest);
        }
        let start = args[2].min(source_layout.height);
        let count = args[3].min(source_layout.height - start).min(layout.height);
        let partial = BitmapLayout32 {
            height: count.max(1),
            ..layout
        };
        let mut transfer = memory::zeroes(count as usize * layout.stride as usize)?;
        let mut pixels = self.surfaces[index]
            .storage
            .read(memory, source_layout.byte_len()?)?;
        if api == GdiApi32::GetDIBits {
            for row in 0..count {
                for x in 0..layout.width {
                    let y = if layout.top_down {
                        start + row
                    } else {
                        source_layout.height - start - row - 1
                    };
                    let color =
                        source_layout.read_pixel(&pixels, &self.surfaces[index].palette, x, y);
                    partial.write_pixel(
                        &mut transfer,
                        &colors,
                        x,
                        if layout.top_down {
                            row
                        } else {
                            count - row - 1
                        },
                        color,
                    );
                }
            }
            header[5] = layout.byte_len()? as u32;
            let info = dib_description(memory, args[5], header, layout, &colors)?;
            memory.write_spans(&[
                (GuestAddress(args[4]), &transfer),
                (GuestAddress(args[5]), &info),
            ])?;
        } else {
            memory.read(GuestAddress(args[4]), &mut transfer)?;
            for row in 0..count {
                for x in 0..layout.width {
                    let color = partial.read_pixel(
                        &transfer,
                        &colors,
                        x,
                        if layout.top_down {
                            row
                        } else {
                            count - row - 1
                        },
                    );
                    let y = if layout.top_down {
                        start + row
                    } else {
                        source_layout.height - start - row - 1
                    };
                    source_layout.write_pixel(
                        &mut pixels,
                        &self.surfaces[index].palette,
                        x,
                        y,
                        color,
                    );
                }
            }
            if count != 0 {
                self.commit_pixels(memory, index, pixels, bounds(source_layout))?;
            }
        }
        Ok(count)
    }
    pub(in crate::process::windows::gdi) fn draw_dib(
        &mut self,
        memory: &mut AddressSpace,
        api: GdiApi32,
        args: &[u32],
    ) -> Result<u32, GdiError32> {
        let stretch = api == GdiApi32::StretchDIBits;
        let (bits, info, usage) = if stretch {
            (args[9], args[10], args[11])
        } else {
            (args[9], args[10], args[11])
        };
        let (layout, palette) = dib_info(memory, info, usage)?;
        if layout.byte_len()? > self.config.bitmap_bytes {
            return Err(GdiError32::Capacity);
        }
        let mut pixels = memory::zeroes(layout.byte_len()?)?;
        let (width, height, source_width, source_height, rop) = if stretch {
            (
                args[3] as i32,
                args[4] as i32,
                args[7] as i32,
                args[8] as i32,
                args[12],
            )
        } else {
            (
                args[3] as i32,
                args[4] as i32,
                args[3] as i32,
                args[4] as i32,
                0x00cc_0020,
            )
        };
        if stretch {
            memory.read(GuestAddress(bits), &mut pixels)?;
        } else {
            let start = args[7].min(layout.height);
            let count = args[8].min(layout.height - start);
            let offset = start as usize * layout.stride as usize;
            let length = count as usize * layout.stride as usize;
            memory.read(GuestAddress(bits), &mut pixels[offset..offset + length])?;
        }
        let state = self.model.state(args[0])?;
        let origin = state.device_point(Point32 {
            x: args[1] as i32,
            y: args[2] as i32,
        })?;
        let end = state.device_point(Point32 {
            x: (args[1] as i32)
                .checked_add(width)
                .ok_or(GdiError32::InvalidRequest)?,
            y: (args[2] as i32)
                .checked_add(height)
                .ok_or(GdiError32::InvalidRequest)?,
        })?;
        let source_x = args[5] as i32;
        let source_y = args[6] as i32;
        let y = if layout.top_down {
            source_y
        } else {
            (layout.height as i32)
                .checked_sub(source_y)
                .and_then(|y| y.checked_sub(source_height))
                .ok_or(GdiError32::InvalidRequest)?
        };
        let source = BlitImage32 {
            layout,
            palette,
            pixels,
            start: Point32 { x: source_x, y },
            end: Point32 {
                x: source_x
                    .checked_add(source_width)
                    .ok_or(GdiError32::InvalidRequest)?,
                y: y.checked_add(source_height)
                    .ok_or(GdiError32::InvalidRequest)?,
            },
            background: state.background_color,
        };
        self.render_blit(
            memory,
            BlitRequest32 {
                dc: args[0],
                origin,
                end,
                source: Some(source),
                rop,
                pattern_only: false,
            },
        )?;
        Ok(if stretch {
            source_height.unsigned_abs()
        } else {
            args[8].min(layout.height.saturating_sub(args[7]))
        })
    }
}

fn dib_description(
    memory: &AddressSpace,
    address: u32,
    header: [u32; 10],
    layout: BitmapLayout32,
    colors: &[u32],
) -> Result<Vec<u8>, GdiError32> {
    if layout.masks.is_some() && header[0] != 40 && header[0] < 52 {
        return Err(GdiError32::InvalidRequest);
    }
    let table_offset = header[0] as usize
        + if header[0] == 40 && layout.masks.is_some() {
            12
        } else {
            0
        };
    let mut info = memory::zeroes(table_offset + colors.len() * 4)?;
    memory.read(GuestAddress(address), &mut info[..header[0] as usize])?;
    for (word, value) in info[..40].chunks_exact_mut(4).zip(header) {
        word.copy_from_slice(&value.to_le_bytes());
    }
    if let Some(masks) = layout.masks {
        for (word, value) in info[40..52].chunks_exact_mut(4).zip(masks) {
            word.copy_from_slice(&value.to_le_bytes());
        }
    }
    for (part, color) in info[table_offset..].chunks_exact_mut(4).zip(colors) {
        part.copy_from_slice(&color_to_quad(*color));
    }
    Ok(info)
}
