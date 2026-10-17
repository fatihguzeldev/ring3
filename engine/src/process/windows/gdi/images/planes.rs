use super::{decoding::dib_pixels, types::*};
use crate::{
    process::windows::gdi::{context::GdiRuntime32, memory},
    windows::{
        gdi::{BitmapLayout32, GdiError32},
        user32::Point32,
    },
};

pub(super) struct ImagePlane32 {
    pub layout: BitmapLayout32,
    pub colors: Vec<u32>,
    pub pixels: Vec<u8>,
}
pub(super) struct ImagePlanes32 {
    pub width: u32,
    pub height: u32,
    pub hotspot: Point32,
    pub alpha: bool,
    pub mask: ImagePlane32,
    pub color: Option<ImagePlane32>,
}
impl GdiRuntime32 {
    pub(super) fn decode_image_planes(
        &self,
        request: GdiImageRequest32<'_>,
    ) -> Result<ImagePlanes32, GdiError32> {
        let source = dib_pixels(request.dib)?;
        if source.layout.top_down || source.layout.height % 2 != 0 {
            return Err(GdiError32::InvalidRequest);
        }
        let _vga = request.vga;
        let source_height = source.layout.height / 2;
        let width = if request.width == 0 {
            source.layout.width
        } else {
            request.width
        };
        let height = if request.height == 0 {
            source_height
        } else {
            request.height
        };
        let mono = source.layout.bits_per_pixel == 1;
        let mask_layout = BitmapLayout32::new(
            i32::try_from(width).map_err(|_| GdiError32::InvalidRequest)?,
            i32::try_from(
                height
                    .checked_mul(if mono { 2 } else { 1 })
                    .ok_or(GdiError32::Capacity)?,
            )
            .map_err(|_| GdiError32::InvalidRequest)?,
            1,
            false,
        )?;
        let color_layout = (!mono)
            .then(|| {
                BitmapLayout32::new(
                    i32::try_from(width).map_err(|_| GdiError32::InvalidRequest)?,
                    i32::try_from(height).map_err(|_| GdiError32::InvalidRequest)?,
                    32,
                    false,
                )
            })
            .transpose()?;
        let size = mask_layout
            .byte_len()?
            .checked_add(color_layout.map_or(Ok(0), BitmapLayout32::byte_len)?)
            .ok_or(GdiError32::Capacity)?;
        if size > self.config.bitmap_bytes {
            return Err(GdiError32::Capacity);
        }
        let mut xor_layout = source.layout;
        xor_layout.height = source_height;
        let xor_len = xor_layout.byte_len()?;
        let and_layout =
            BitmapLayout32::new(source.layout.width as i32, source_height as i32, 1, true)?;
        let and_len = and_layout.byte_len()?;
        let xor = source
            .pixels
            .get(..xor_len)
            .ok_or(GdiError32::InvalidRequest)?;
        let and = source
            .pixels
            .get(
                xor_len
                    ..xor_len
                        .checked_add(and_len)
                        .ok_or(GdiError32::InvalidRequest)?,
            )
            .ok_or(GdiError32::InvalidRequest)?;
        let mut colors = source.colors;
        self.image_palette(
            &mut colors,
            xor_layout,
            xor,
            request.transparent,
            request.map3d,
        )?;
        let alpha = !mono
            && xor_layout.bits_per_pixel == 32
            && xor_layout.masks.is_none()
            && (0..xor_layout.height).any(|y| {
                (0..xor_layout.width).any(|x| xor[xor_layout.row(y) + x as usize * 4 + 3] != 0)
            });
        let mut mask = ImagePlane32 {
            layout: mask_layout,
            colors: vec![0, 0xffffff],
            pixels: memory::zeroes(mask_layout.byte_len()?)?,
        };
        let mut color = color_layout
            .map(|layout| {
                Ok::<ImagePlane32, GdiError32>(ImagePlane32 {
                    layout,
                    colors: Vec::new(),
                    pixels: memory::zeroes(layout.byte_len()?)?,
                })
            })
            .transpose()?;
        for y in 0..height {
            let sy = (u64::from(y) * u64::from(source_height) / u64::from(height)) as u32;
            for x in 0..width {
                let sx = (u64::from(x) * u64::from(xor_layout.width) / u64::from(width)) as u32;
                let value = and_layout.read_pixel(and, &[0, 0xffffff], sx, sy);
                mask.layout
                    .write_pixel(&mut mask.pixels, &mask.colors, x, y, value);
                let value = xor_layout.read_pixel(xor, &colors, sx, sy);
                let value = if request.monochrome {
                    [0, 0xffffff][crate::windows::gdi::nearest_palette_index32(
                        &[0, 0xffffff],
                        value,
                    )
                    .unwrap()]
                } else {
                    value
                };
                if let Some(color) = &mut color {
                    color
                        .layout
                        .write_pixel(&mut color.pixels, &color.colors, x, y, value);
                    if alpha {
                        color.pixels[color.layout.row(y) + x as usize * 4 + 3] =
                            xor[xor_layout.row(sy) + sx as usize * 4 + 3];
                    }
                } else {
                    mask.layout
                        .write_pixel(&mut mask.pixels, &mask.colors, x, y + height, value);
                }
            }
        }
        let hotspot = if request.kind == GdiImageKind32::Icon {
            Point32 {
                x: width as i32 / 2,
                y: height as i32 / 2,
            }
        } else {
            if request.hotspot.x < 0
                || request.hotspot.y < 0
                || request.hotspot.x as u32 >= xor_layout.width
                || request.hotspot.y as u32 >= source_height
            {
                return Err(GdiError32::InvalidRequest);
            }
            Point32 {
                x: (u64::from(request.hotspot.x as u32) * u64::from(width)
                    / u64::from(xor_layout.width)) as i32,
                y: (u64::from(request.hotspot.y as u32) * u64::from(height)
                    / u64::from(source_height)) as i32,
            }
        };
        Ok(ImagePlanes32 {
            width,
            height,
            hotspot,
            alpha,
            mask,
            color,
        })
    }
    pub(super) fn image_palette(
        &self,
        colors: &mut [u32],
        layout: BitmapLayout32,
        pixels: &[u8],
        transparent: bool,
        map3d: bool,
    ) -> Result<(), GdiError32> {
        if colors.is_empty() {
            return Ok(());
        }
        if transparent {
            let at = layout.row(0);
            let index = match layout.bits_per_pixel {
                1 => usize::from(pixels[at] >> 7),
                4 => usize::from(pixels[at] >> 4),
                8 => usize::from(pixels[at]),
                _ => return Err(GdiError32::InvalidRequest),
            };
            *colors.get_mut(index).ok_or(GdiError32::InvalidRequest)? =
                self.system_color(if map3d { 15 } else { 5 });
        } else if map3d {
            for color in colors {
                *color = match *color {
                    0x808080 => self.system_color(16),
                    0xc0c0c0 => self.system_color(15),
                    0xdfdfdf => self.system_color(22),
                    _ => *color,
                };
            }
        }
        Ok(())
    }
}
