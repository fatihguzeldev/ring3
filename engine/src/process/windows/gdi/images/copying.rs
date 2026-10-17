use super::{
    planes::{ImagePlane32, ImagePlanes32},
    preparation::PreparedGdiImage32,
    types::*,
};
use crate::{
    memory::AddressSpace,
    process::windows::gdi::{context::GdiRuntime32, memory},
    windows::{
        gdi::{BitmapLayout32, GdiError32, nearest_palette_index32},
        user32::Point32,
    },
};
impl GdiRuntime32 {
    pub(in crate::process) fn prepare_copy_image(
        &self,
        memory: &mut AddressSpace,
        handle: u32,
        options: ResourceBitmapOptions32,
        replace: Option<u32>,
    ) -> Result<PreparedGdiImage32, GdiError32> {
        if options.dib_section || options.transparent || options.map3d || options.vga {
            return Err(GdiError32::InvalidRequest);
        }
        let source = self.image_info(handle)?;
        let owner = self.image_owner(handle)?;
        let width = if options.width == 0 {
            source.width
        } else {
            options.width
        };
        let height = if options.height == 0 {
            source.height
        } else {
            options.height
        };
        let width_i = i32::try_from(width).map_err(|_| GdiError32::InvalidRequest)?;
        let height_i = i32::try_from(height).map_err(|_| GdiError32::InvalidRequest)?;
        let mask_layout = BitmapLayout32::new(
            width_i,
            height_i
                .checked_mul(if source.monochrome { 2 } else { 1 })
                .ok_or(GdiError32::Capacity)?,
            1,
            false,
        )?;
        let color_layout = (!source.monochrome)
            .then(|| BitmapLayout32::new(width_i, height_i, 32, false))
            .transpose()?;
        let size = mask_layout
            .byte_len()?
            .checked_add(
                color_layout
                    .map(|layout| layout.byte_len())
                    .transpose()?
                    .unwrap_or(0),
            )
            .ok_or(GdiError32::Capacity)?;
        if size > self.config.bitmap_bytes {
            return Err(GdiError32::Capacity);
        }
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
            let sy = (u64::from(y) * u64::from(source.height) / u64::from(height)) as u32;
            for x in 0..width {
                let sx = (u64::from(x) * u64::from(source.width) / u64::from(width)) as u32;
                let and =
                    source
                        .mask
                        .layout
                        .read_pixel(source.mask.pixels, source.mask.palette, sx, sy);
                mask.layout
                    .write_pixel(&mut mask.pixels, &mask.colors, x, y, and);
                if let (Some(destination), Some(input)) = (&mut color, source.color) {
                    let value = input.layout.read_pixel(input.pixels, input.palette, sx, sy);
                    let value = if options.monochrome {
                        [0, 0xffffff][nearest_palette_index32(&[0, 0xffffff], value).unwrap()]
                    } else {
                        value
                    };
                    destination.layout.write_pixel(
                        &mut destination.pixels,
                        &destination.colors,
                        x,
                        y,
                        value,
                    );
                    destination.pixels[destination.layout.row(y) + x as usize * 4 + 3] =
                        input.pixels[input.layout.row(sy) + sx as usize * 4 + 3];
                } else {
                    let value = source.mask.layout.read_pixel(
                        source.mask.pixels,
                        source.mask.palette,
                        sx,
                        sy + source.height,
                    );
                    mask.layout
                        .write_pixel(&mut mask.pixels, &mask.colors, x, y + height, value);
                }
            }
        }
        let hotspot = if source.kind == GdiImageKind32::Icon {
            Point32 {
                x: width_i / 2,
                y: height_i / 2,
            }
        } else {
            Point32 {
                x: (u64::from(source.hotspot.x as u32) * u64::from(width) / u64::from(source.width))
                    as i32,
                y: (u64::from(source.hotspot.y as u32) * u64::from(height)
                    / u64::from(source.height)) as i32,
            }
        };
        self.prepare_image_planes(
            memory,
            source.kind,
            ImagePlanes32 {
                width,
                height,
                hotspot,
                alpha: source.alpha,
                mask,
                color,
            },
            replace,
            Some(owner),
        )
    }
}
