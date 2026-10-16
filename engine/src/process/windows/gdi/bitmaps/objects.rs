use super::super::{
    context::GdiRuntime32,
    memory,
    storage::{Storage32, SurfaceKey32},
};
use crate::{
    memory::{
        Access, AddressSpace, GuestAddress, PAGE_SIZE, PageInitialization, Permissions, WordWrite32,
    },
    windows::gdi::{BitmapLayout32, GdiApi32, GdiError32, GdiObject32},
};

pub(in crate::process::windows::gdi) fn palette(bits: u16) -> Vec<u32> {
    if bits == 1 {
        vec![0, 0x00ff_ffff]
    } else if bits <= 8 {
        (0..1u32 << bits)
            .map(|n| {
                let c = n * 255 / ((1 << bits) - 1);
                c | c << 8 | c << 16
            })
            .collect()
    } else {
        Vec::new()
    }
}
impl GdiRuntime32 {
    pub(in crate::process::windows::gdi) fn create_bitmap(
        &mut self,
        memory: &AddressSpace,
        layout: BitmapLayout32,
        input: u32,
    ) -> Result<u32, GdiError32> {
        let prepared = self.model.prepare_object(GdiObject32::Bitmap(layout))?;
        let mut surface = self.prepare_surface(
            SurfaceKey32::Bitmap(prepared.handle()),
            layout,
            palette(layout.bits_per_pixel),
        )?;
        if input != 0
            && let Storage32::Owned(bytes) = &mut surface.storage
        {
            memory.read(GuestAddress(input), bytes)?;
        }
        let handle = self.model.commit_new(prepared);
        self.publish_surface(surface);
        Ok(handle)
    }
    pub(in crate::process::windows::gdi) fn compatible_bitmap(
        &mut self,
        memory: &mut AddressSpace,
        dc: u32,
        width: i32,
        height: i32,
    ) -> Result<u32, GdiError32> {
        let index = self.dc_surface(dc)?;
        let source = self.surfaces[index].layout;
        if width < 0 || height < 0 {
            return Err(GdiError32::InvalidRequest);
        }
        if width == 0 || height == 0 {
            return self.create_bitmap(memory, BitmapLayout32::new(1, 1, 1, false)?, 0);
        }
        if source.dib {
            self.surfaces[index].storage.check(memory)?;
            let colors = self.surfaces[index].palette.clone();
            let mut layout = BitmapLayout32::new(
                width,
                if source.top_down { -height } else { height },
                source.bits_per_pixel,
                true,
            )?;
            layout.masks = source.masks;
            return self.allocate_dib(memory, layout, colors, None);
        }
        self.create_bitmap(
            memory,
            BitmapLayout32::new(width, height, source.bits_per_pixel, false)?,
            0,
        )
    }
    pub(in crate::process::windows::gdi) fn create_dib(
        &mut self,
        memory: &mut AddressSpace,
        args: &[u32],
    ) -> Result<u32, GdiError32> {
        memory.check_span(GuestAddress(args[3]), 4, Access::Write)?;
        let (layout, colors) = dib_info(memory, args[1], args[2])?;
        if args[4] != 0 {
            return Err(GdiError32::Unsupported);
        }
        self.allocate_dib(memory, layout, colors, Some(args[3]))
    }
    fn allocate_dib(
        &mut self,
        memory: &mut AddressSpace,
        layout: BitmapLayout32,
        colors: Vec<u32>,
        output: Option<u32>,
    ) -> Result<u32, GdiError32> {
        let prepared = self.model.prepare_object(GdiObject32::Bitmap(layout))?;
        let mut surface =
            self.prepare_surface(SurfaceKey32::Bitmap(prepared.handle()), layout, colors)?;
        let length = layout.byte_len()?;
        let range = memory.find_free_range(
            self.bounds,
            u32::try_from(length.div_ceil(PAGE_SIZE as usize)).map_err(|_| GdiError32::Capacity)?,
            PAGE_SIZE,
        )?;
        let address = GuestAddress(range.first as u32 * PAGE_SIZE);
        let mut pages = Vec::new();
        pages
            .try_reserve_exact(range.count)
            .map_err(|_| GdiError32::Allocation)?;
        for index in 0..range.count {
            pages.push(PageInitialization {
                address: GuestAddress(address.0 + index as u32 * PAGE_SIZE),
                permissions: Permissions::READ_WRITE,
                bytes: &[],
            });
        }
        let words = output.map(|output| WordWrite32 {
            address: GuestAddress(output),
            value: address.0,
        });
        memory.observe_mapping_incarnations(range)?;
        memory.install_pages_with_word_writes(&pages, words.as_slice())?;
        surface.storage = Storage32::Dib {
            range,
            address: address.0,
            identity: memory.identity(),
            mapping: memory.mapping_incarnation_at(address).unwrap(),
            section: None,
        };
        let handle = self.model.commit_new(prepared);
        self.publish_surface(surface);
        Ok(handle)
    }
    pub(in crate::process::windows::gdi) fn bitmap_bits(
        &mut self,
        memory: &mut AddressSpace,
        api: GdiApi32,
        args: &[u32],
    ) -> Result<u32, GdiError32> {
        let index = self.surface_index(SurfaceKey32::Bitmap(args[0]))?;
        let layout = self.surfaces[index].layout;
        let count = (args[1] as usize).min(layout.byte_len()?);
        if api == GdiApi32::GetBitmapBits {
            if args[2] == 0 {
                return Ok(layout.byte_len()? as u32);
            }
            let bytes = self.surfaces[index]
                .storage
                .read(memory, layout.byte_len()?)?;
            memory.write(GuestAddress(args[2]), &bytes[..count])?;
        } else {
            if args[2] == 0 {
                return Err(GdiError32::InvalidRequest);
            }
            let mut bytes = self.surfaces[index]
                .storage
                .read(memory, layout.byte_len()?)?;
            memory.read(GuestAddress(args[2]), &mut bytes[..count])?;
            self.commit_pixels(memory, index, bytes, bounds(layout))?;
        }
        Ok(count as u32)
    }
    pub(in crate::process::windows::gdi) fn dib_colors(
        &mut self,
        memory: &mut AddressSpace,
        api: GdiApi32,
        args: &[u32],
    ) -> Result<u32, GdiError32> {
        let index = self.dc_surface(args[0])?;
        let surface = &mut self.surfaces[index];
        if !surface.layout.dib || surface.layout.bits_per_pixel > 8 {
            return Ok(0);
        }
        let start = args[1] as usize;
        if start >= surface.palette.len() {
            return Ok(0);
        }
        let count = (args[2] as usize).min(surface.palette.len() - start);
        let mut bytes = memory::zeroes(count * 4)?;
        if api == GdiApi32::GetDIBColorTable {
            for (color, part) in surface.palette[start..start + count]
                .iter()
                .zip(bytes.chunks_exact_mut(4))
            {
                part.copy_from_slice(&color_to_quad(*color));
            }
            memory.write(GuestAddress(args[3]), &bytes)?;
        } else {
            memory.read(GuestAddress(args[3]), &mut bytes)?;
            for (color, part) in surface.palette[start..start + count]
                .iter_mut()
                .zip(bytes.chunks_exact(4))
            {
                *color = quad_to_color(part);
            }
        }
        Ok(count as u32)
    }
}
pub(in crate::process::windows::gdi) fn dib_info(
    memory: &AddressSpace,
    address: u32,
    usage: u32,
) -> Result<(BitmapLayout32, Vec<u32>), GdiError32> {
    if usage != 0 {
        return Err(GdiError32::Unsupported);
    }
    let header: [u32; 10] = memory::words(memory, address)?;
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
        let masks = memory::words(
            memory,
            address.checked_add(40).ok_or(GdiError32::InvalidRequest)?,
        )?;
        layout = layout.with_masks(masks)?;
        if header[0] == 40 {
            table_offset += 12;
        }
    }
    let count = if bits <= 8 {
        if header[8] == 0 {
            1u32 << bits
        } else {
            header[8]
        }
    } else {
        header[8]
    };
    if count > 256 || (bits <= 8 && count > 1u32 << bits) {
        return Err(GdiError32::InvalidRequest);
    }
    let mut colors = Vec::new();
    colors
        .try_reserve_exact(count as usize)
        .map_err(|_| GdiError32::Allocation)?;
    if count != 0 {
        let mut bytes = memory::zeroes(count as usize * 4)?;
        memory.read(
            GuestAddress(
                address
                    .checked_add(table_offset)
                    .ok_or(GdiError32::InvalidRequest)?,
            ),
            &mut bytes,
        )?;
        colors.extend(bytes.chunks_exact(4).map(quad_to_color));
    }
    Ok((layout, colors))
}
pub(in crate::process::windows::gdi) fn bounds(
    layout: BitmapLayout32,
) -> crate::windows::user32::Rect32 {
    crate::windows::user32::Rect32 {
        left: 0,
        top: 0,
        right: layout.width as i32,
        bottom: layout.height as i32,
    }
}
pub(in crate::process::windows::gdi) fn quad_to_color(bytes: &[u8]) -> u32 {
    u32::from(bytes[2]) | u32::from(bytes[1]) << 8 | u32::from(bytes[0]) << 16
}
pub(in crate::process::windows::gdi) fn color_to_quad(color: u32) -> [u8; 4] {
    [(color >> 16) as u8, (color >> 8) as u8, color as u8, 0]
}
