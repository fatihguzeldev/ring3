use super::{
    decoding::{DibPixels32, dib_pixels},
    types::*,
};
use crate::{
    memory::{AddressSpace, GuestAddress, PAGE_SIZE, PageInitialization, PageRange, Permissions},
    process::windows::gdi::{
        context::GdiRuntime32,
        memory,
        storage::{Storage32, Surface32, SurfaceKey32},
    },
    windows::gdi::{BitmapLayout32, GdiError32, GdiObject32, PreparedGdiObject32},
};
#[derive(Clone, Copy)]
struct BitmapOwner32 {
    handle: u32,
    id: u64,
    revision: u64,
}
pub(in crate::process) struct PreparedGdiBitmap32 {
    identity: u64,
    version: u64,
    next_surface: u64,
    object: PreparedGdiObject32,
    surface: Surface32,
    range: Option<PageRange>,
    retire: Option<BitmapOwner32>,
    source: Option<BitmapOwner32>,
}

impl GdiRuntime32 {
    pub(in crate::process) fn bitmap_info(
        &self,
        ram: &AddressSpace,
        handle: u32,
    ) -> Result<GdiBitmapInfo32, GdiError32> {
        let GdiObject32::Bitmap(layout) = self.model.object(handle)? else {
            return Err(GdiError32::InvalidHandle);
        };
        let index = self.surface_index(SurfaceKey32::Bitmap(handle))?;
        self.surfaces[index].storage.check(ram)?;
        Ok(GdiBitmapInfo32 {
            width: layout.width,
            height: layout.height,
            monochrome: layout.bits_per_pixel == 1,
            dib_section: layout.dib,
        })
    }
    pub(in crate::process) fn prepare_resource_bitmap(
        &self,
        ram: &mut AddressSpace,
        source: GdiDibSource32<'_>,
        options: ResourceBitmapOptions32,
        replace: Option<u32>,
    ) -> Result<PreparedGdiBitmap32, GdiError32> {
        let mut input = dib_pixels(source)?;
        input.check_pixels()?;
        self.image_palette(
            &mut input.colors,
            input.layout,
            input.pixels,
            options.transparent,
            options.map3d,
        )?;
        self.prepare_bitmap_pixels(ram, input, options, replace, None)
    }
    pub(in crate::process) fn prepare_copy_bitmap(
        &self,
        ram: &mut AddressSpace,
        handle: u32,
        options: ResourceBitmapOptions32,
        replace: Option<u32>,
    ) -> Result<PreparedGdiBitmap32, GdiError32> {
        let owner = self.bitmap_owner(ram, handle)?;
        let surface = &self.surfaces[self.surface_index(SurfaceKey32::Bitmap(handle))?];
        let pixels = surface.storage.read(ram, surface.layout.byte_len()?)?;
        let mut colors = Vec::new();
        colors
            .try_reserve_exact(surface.palette.len())
            .map_err(|_| GdiError32::Allocation)?;
        colors.extend_from_slice(&surface.palette);
        self.prepare_bitmap_pixels(
            ram,
            DibPixels32 {
                layout: surface.layout,
                colors,
                pixels: &pixels,
            },
            options,
            replace,
            Some(owner),
        )
    }
    fn bitmap_owner(&self, ram: &AddressSpace, handle: u32) -> Result<BitmapOwner32, GdiError32> {
        self.bitmap_info(ram, handle)?;
        let surface = &self.surfaces[self.surface_index(SurfaceKey32::Bitmap(handle))?];
        Ok(BitmapOwner32 {
            handle,
            id: surface.id,
            revision: surface.revision,
        })
    }
    fn check_bitmap_owner(
        &self,
        ram: &AddressSpace,
        owner: BitmapOwner32,
    ) -> Result<(), GdiError32> {
        let current = self.bitmap_owner(ram, owner.handle)?;
        if current.id != owner.id || current.revision != owner.revision {
            return Err(GdiError32::InvalidRequest);
        }
        Ok(())
    }
    fn prepare_bitmap_pixels(
        &self,
        ram: &mut AddressSpace,
        source: DibPixels32<'_>,
        options: ResourceBitmapOptions32,
        replace: Option<u32>,
        source_owner: Option<BitmapOwner32>,
    ) -> Result<PreparedGdiBitmap32, GdiError32> {
        let width = if options.width == 0 {
            source.layout.width
        } else {
            options.width
        };
        let height = if options.height == 0 {
            source.layout.height
        } else {
            options.height
        };
        let monochrome = !options.dib_section
            && (options.monochrome
                || (source.layout.bits_per_pixel == 1 && source.colors == [0, 0xffffff]));
        let bits = if options.dib_section {
            source.layout.bits_per_pixel
        } else if monochrome {
            1
        } else {
            32
        };
        let signed_height = i32::try_from(height).map_err(|_| GdiError32::InvalidRequest)?;
        let mut layout = BitmapLayout32::new(
            i32::try_from(width).map_err(|_| GdiError32::InvalidRequest)?,
            if options.dib_section && source.layout.top_down {
                -signed_height
            } else {
                signed_height
            },
            bits,
            options.dib_section,
        )?;
        if options.dib_section {
            layout.masks = source.layout.masks;
        }
        let mut colors = Vec::new();
        let palette: &[u32] = if options.dib_section {
            source.colors.as_slice()
        } else if monochrome {
            &[0, 0xffffff]
        } else {
            &[]
        };
        colors
            .try_reserve_exact(palette.len())
            .map_err(|_| GdiError32::Allocation)?;
        colors.extend_from_slice(palette);
        let _vga = options.vga;
        let retire = replace
            .map(|handle| self.bitmap_owner(ram, handle))
            .transpose()?;
        let old = retire
            .map(|owner| self.surface_index(SurfaceKey32::Bitmap(owner.handle)))
            .transpose()?;
        if old.is_some_and(|index| self.surfaces[index].storage.section().is_some()) {
            return Err(GdiError32::Unsupported);
        }
        let old_size = old
            .map(|index| self.surfaces[index].layout.byte_len())
            .transpose()?
            .unwrap_or(0);
        let size = layout.byte_len()?;
        if size
            > self
                .config
                .bitmap_bytes
                .saturating_sub(self.bytes - old_size + self.model.palette_bytes())
            || self.surfaces.len() - usize::from(retire.is_some())
                == self.config.objects + self.config.dcs
        {
            return Err(GdiError32::Capacity);
        }
        let object = self
            .model
            .prepare_object_replacing(GdiObject32::Bitmap(layout), replace)?;
        self.next_surface
            .checked_add(1)
            .ok_or(GdiError32::Exhausted)?;
        let mut pixels = memory::zeroes(size)?;
        for y in 0..height {
            let sy = (u64::from(y) * u64::from(source.layout.height) / u64::from(height)) as u32;
            for x in 0..width {
                let sx = (u64::from(x) * u64::from(source.layout.width) / u64::from(width)) as u32;
                if options.dib_section && bits <= 8 {
                    let index = read_index(source.layout, source.pixels, sx, sy);
                    write_index(layout, &mut pixels, x, y, index);
                } else {
                    let color = source
                        .layout
                        .read_pixel(source.pixels, &source.colors, sx, sy);
                    layout.write_pixel(&mut pixels, &colors, x, y, color);
                    if bits == 32
                        && source.layout.bits_per_pixel == 32
                        && layout.masks == source.layout.masks
                    {
                        pixels[layout.row(y) + x as usize * 4 + 3] =
                            source.pixels[source.layout.row(sy) + sx as usize * 4 + 3];
                    }
                }
            }
        }
        let range = if options.dib_section {
            let range = ram.find_free_range(
                self.bounds,
                u32::try_from(size.div_ceil(PAGE_SIZE as usize))
                    .map_err(|_| GdiError32::Capacity)?,
                PAGE_SIZE,
            )?;
            ram.observe_mapping_incarnations(range)?;
            Some(range)
        } else {
            None
        };
        let surface = Surface32 {
            key: SurfaceKey32::Bitmap(object.handle()),
            id: self.next_surface,
            revision: 1,
            damage: Default::default(),
            layout,
            palette: colors,
            storage: Storage32::Owned(pixels),
        };
        Ok(PreparedGdiBitmap32 {
            identity: ram.identity(),
            version: ram.transaction_version(),
            next_surface: self.next_surface,
            object,
            surface,
            range,
            retire,
            source: source_owner,
        })
    }
    pub(in crate::process) fn check_resource_bitmap(
        &self,
        ram: &AddressSpace,
        receipt: &PreparedGdiBitmap32,
    ) -> Result<(), GdiError32> {
        if ram.identity() != receipt.identity
            || ram.transaction_version() != receipt.version
            || self.next_surface != receipt.next_surface
        {
            return Err(GdiError32::InvalidRequest);
        }
        self.model.check_prepared_new(&receipt.object)?;
        if let Some(source) = receipt.source {
            self.check_bitmap_owner(ram, source)?;
        }
        if let Some(retire) = receipt.retire {
            self.check_bitmap_owner(ram, retire)?;
        }
        if let Some(range) = receipt.range {
            for page in range.first..range.first + range.count {
                if ram
                    .permissions_at(GuestAddress(page as u32 * PAGE_SIZE))
                    .is_ok()
                {
                    return Err(GdiError32::InvalidRequest);
                }
            }
        }
        let old = receipt
            .retire
            .map(|owner| self.surface_index(SurfaceKey32::Bitmap(owner.handle)))
            .transpose()?;
        let old_size = old
            .map(|index| self.surfaces[index].layout.byte_len())
            .transpose()?
            .unwrap_or(0);
        if self.surfaces.len() - usize::from(receipt.retire.is_some())
            >= self.config.objects + self.config.dcs
            || receipt.surface.layout.byte_len()?
                > self
                    .config
                    .bitmap_bytes
                    .saturating_sub(self.bytes - old_size + self.model.palette_bytes())
        {
            return Err(GdiError32::Capacity);
        }
        let old_range = old.and_then(|index| self.surfaces[index].storage.range());
        ram.ensure_transaction_versions(
            u64::from(receipt.range.is_some()) + u64::from(old_range.is_some()),
        )?;
        Ok(())
    }
    pub(in crate::process) fn commit_resource_bitmap(
        &mut self,
        ram: &mut AddressSpace,
        mut receipt: PreparedGdiBitmap32,
    ) -> Result<u32, GdiError32> {
        self.check_resource_bitmap(ram, &receipt)?;
        let old = receipt
            .retire
            .map(|owner| self.surface_index(SurfaceKey32::Bitmap(owner.handle)))
            .transpose()?;
        let old_range = old.and_then(|index| self.surfaces[index].storage.range());
        if let Some(range) = receipt.range {
            let Storage32::Owned(bytes) = &receipt.surface.storage else {
                unreachable!()
            };
            let base = range.first as u32 * PAGE_SIZE;
            let mut pages = Vec::new();
            pages
                .try_reserve_exact(range.count)
                .map_err(|_| GdiError32::Allocation)?;
            for page in 0..range.count {
                let at = page * PAGE_SIZE as usize;
                pages.push(PageInitialization {
                    address: GuestAddress(base + page as u32 * PAGE_SIZE),
                    permissions: Permissions::READ_WRITE,
                    bytes: &bytes[at..bytes.len().min(at + PAGE_SIZE as usize)],
                });
            }
            ram.install_pages_with_word_writes(&pages, &[])?;
            receipt.surface.storage = Storage32::Dib {
                range,
                address: base,
                identity: ram.identity(),
                mapping: ram.mapping_incarnation_at(GuestAddress(base)).unwrap(),
                section: None,
            };
        }
        if let Some(range) = old_range {
            ram.unmap_ranges_with_word_writes(&[range], &[])
                .expect("disjoint bitmap replacement and version headroom were preflighted");
        }
        if let Some(index) = old {
            let old = self.surfaces.remove(index);
            self.bytes -= old.layout.byte_len().unwrap();
        }
        let handle = self.model.commit_new(receipt.object);
        self.publish_surface(receipt.surface);
        Ok(handle)
    }
}
fn read_index(layout: BitmapLayout32, bytes: &[u8], x: u32, y: u32) -> u8 {
    let row = layout.row(y);
    match layout.bits_per_pixel {
        1 => (bytes[row + x as usize / 8] >> (7 - x % 8)) & 1,
        4 => (bytes[row + x as usize / 2] >> if x % 2 == 0 { 4 } else { 0 }) & 15,
        8 => bytes[row + x as usize],
        _ => unreachable!(),
    }
}
fn write_index(layout: BitmapLayout32, bytes: &mut [u8], x: u32, y: u32, value: u8) {
    let row = layout.row(y);
    let (at, shift, mask) = match layout.bits_per_pixel {
        1 => (row + x as usize / 8, 7 - x % 8, 1),
        4 => (row + x as usize / 2, if x % 2 == 0 { 4 } else { 0 }, 15),
        8 => {
            bytes[row + x as usize] = value;
            return;
        }
        _ => unreachable!(),
    };
    bytes[at] = (bytes[at] & !(mask << shift)) | ((value & mask) << shift);
}
