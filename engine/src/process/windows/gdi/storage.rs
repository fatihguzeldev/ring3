use super::{GdiFrame32, context::GdiRuntime32};
use crate::{
    memory::{AddressSpace, GuestAddress, PAGE_SIZE, PageRange},
    windows::{
        gdi::{BitmapLayout32, GdiError32},
        user32::Rect32,
    },
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SurfaceKey32 {
    Bitmap(u32),
    PatternBrush(u32),
    ImageMask(u32),
    ImageColor(u32),
    Default(u32),
    Window { window: u32, whole: bool },
}
pub(super) enum Storage32 {
    Owned(Vec<u8>),
    Dib {
        range: PageRange,
        address: u32,
        identity: u64,
        mapping: u64,
        section: Option<crate::process::windows::files::DibSection32>,
    },
}
impl Storage32 {
    pub(super) fn range(&self) -> Option<PageRange> {
        match self {
            Self::Dib {
                range,
                section: None,
                ..
            } => Some(*range),
            _ => None,
        }
    }
    pub(super) fn address(&self) -> Option<u32> {
        if let Self::Dib { address, .. } = self {
            Some(*address)
        } else {
            None
        }
    }
    pub(super) fn section(&self) -> Option<crate::process::windows::files::DibSection32> {
        if let Self::Dib { section, .. } = self {
            *section
        } else {
            None
        }
    }
    pub(super) fn check(&self, memory: &AddressSpace) -> Result<(), GdiError32> {
        if let Self::Dib {
            range,
            identity,
            mapping,
            ..
        } = self
        {
            if *identity != memory.identity() {
                return Err(GdiError32::InvalidRequest);
            }
            for page in range.first..range.first + range.count {
                if memory.mapping_incarnation_at(GuestAddress(page as u32 * PAGE_SIZE))? != *mapping
                {
                    return Err(GdiError32::InvalidRequest);
                }
            }
        }
        Ok(())
    }
    pub(super) fn read(&self, memory: &AddressSpace, length: usize) -> Result<Vec<u8>, GdiError32> {
        self.check(memory)?;
        let mut bytes = super::memory::zeroes(length)?;
        match self {
            Self::Owned(input) => bytes.copy_from_slice(input),
            Self::Dib { address, .. } => memory.read(GuestAddress(*address), &mut bytes)?,
        }
        Ok(bytes)
    }
    pub(super) fn write(
        &mut self,
        memory: &mut AddressSpace,
        bytes: Vec<u8>,
    ) -> Result<(), GdiError32> {
        self.check(memory)?;
        match self {
            Self::Owned(output) => *output = bytes,
            Self::Dib { address, .. } => memory.write(GuestAddress(*address), &bytes)?,
        }
        Ok(())
    }
}
pub(super) struct Surface32 {
    pub key: SurfaceKey32,
    pub id: u64,
    pub revision: u64,
    pub damage: Rect32,
    pub layout: BitmapLayout32,
    pub palette: Vec<u32>,
    pub storage: Storage32,
}
impl Surface32 {
    pub(super) fn new(
        key: SurfaceKey32,
        id: u64,
        layout: BitmapLayout32,
        palette: Vec<u32>,
    ) -> Result<Self, GdiError32> {
        Ok(Self {
            key,
            id,
            revision: 1,
            damage: Rect32::default(),
            layout,
            palette,
            storage: Storage32::Owned(super::memory::zeroes(layout.byte_len()?)?),
        })
    }
}
impl GdiRuntime32 {
    pub(super) fn prepare_surface(
        &self,
        key: SurfaceKey32,
        layout: BitmapLayout32,
        palette: Vec<u32>,
    ) -> Result<Surface32, GdiError32> {
        let length = layout.byte_len()?;
        if self.surfaces.len() == self.config.objects + self.config.dcs
            || length
                > self
                    .config
                    .bitmap_bytes
                    .saturating_sub(self.bytes + self.model.palette_bytes())
        {
            return Err(GdiError32::Capacity);
        }
        self.next_surface
            .checked_add(1)
            .ok_or(GdiError32::Exhausted)?;
        Surface32::new(key, self.next_surface, layout, palette)
    }
    pub(super) fn publish_surface(&mut self, surface: Surface32) {
        self.next_surface += 1;
        self.bytes += surface.layout.byte_len().unwrap();
        self.surfaces.push(surface);
    }
    pub(super) fn surface_index(&self, key: SurfaceKey32) -> Result<usize, GdiError32> {
        self.surfaces
            .iter()
            .position(|surface| surface.key == key)
            .ok_or(GdiError32::InvalidHandle)
    }
    pub(super) fn dc_surface(&self, dc: u32) -> Result<usize, GdiError32> {
        match self.model.dc_kind(dc)? {
            crate::windows::gdi::DcKind32::Memory { .. } => {
                let bitmap = self.model.state(dc)?.bitmap;
                self.surface_index(if self.model.is_stock(bitmap) {
                    SurfaceKey32::Default(dc)
                } else {
                    SurfaceKey32::Bitmap(bitmap)
                })
            }
            crate::windows::gdi::DcKind32::Display {
                window, window_dc, ..
            } => self.surface_index(SurfaceKey32::Window {
                window,
                whole: window_dc,
            }),
            crate::windows::gdi::DcKind32::Screen => self.surface_index(SurfaceKey32::Window {
                window: 0,
                whole: false,
            }),
        }
    }
    pub(super) fn commit_pixels(
        &mut self,
        memory: &mut AddressSpace,
        index: usize,
        pixels: Vec<u8>,
        damage: Rect32,
    ) -> Result<(), GdiError32> {
        self.commit_pixels_with_spans(memory, index, pixels, damage, &[])
    }
    pub(super) fn commit_pixels_with_spans(
        &mut self,
        memory: &mut AddressSpace,
        index: usize,
        pixels: Vec<u8>,
        damage: Rect32,
        spans: &[(GuestAddress, &[u8])],
    ) -> Result<(), GdiError32> {
        if let SurfaceKey32::Bitmap(handle) = self.surfaces[index].key {
            self.model.check_write_object(handle)?;
        }
        let surface = &mut self.surfaces[index];
        let revision = surface
            .revision
            .checked_add(1)
            .ok_or(GdiError32::Exhausted)?;
        surface.storage.check(memory)?;
        if spans.is_empty() {
            surface.storage.write(memory, pixels)?;
        } else {
            match &mut surface.storage {
                Storage32::Owned(output) => {
                    memory.write_spans(spans)?;
                    *output = pixels;
                }
                Storage32::Dib { address, .. } => {
                    let mut writes = Vec::new();
                    writes
                        .try_reserve_exact(spans.len() + 1)
                        .map_err(|_| GdiError32::Allocation)?;
                    writes.push((GuestAddress(*address), pixels.as_slice()));
                    writes.extend_from_slice(spans);
                    memory.write_spans(&writes)?;
                }
            }
        }
        surface.revision = revision;
        surface.damage = if surface.damage.left >= surface.damage.right
            || surface.damage.top >= surface.damage.bottom
        {
            damage
        } else {
            Rect32 {
                left: surface.damage.left.min(damage.left),
                top: surface.damage.top.min(damage.top),
                right: surface.damage.right.max(damage.right),
                bottom: surface.damage.bottom.max(damage.bottom),
            }
        };
        Ok(())
    }
    pub(in crate::process) fn frame(&self, index: usize) -> Option<GdiFrame32> {
        let surface = self
            .surfaces
            .iter()
            .filter(|surface| {
                matches!(surface.key, SurfaceKey32::Window { .. })
                    && surface.damage.left < surface.damage.right
                    && surface.damage.top < surface.damage.bottom
            })
            .nth(index)?;
        let SurfaceKey32::Window { window, whole } = surface.key else {
            unreachable!()
        };
        Some(GdiFrame32 {
            format: 1,
            id: surface.id,
            revision: surface.revision,
            window,
            window_dc: whole,
            width: surface.layout.width,
            height: surface.layout.height,
            stride: surface.layout.stride,
            bytes: surface.layout.byte_len().ok()? as u32,
            damage: surface.damage,
        })
    }
    pub(in crate::process) fn read_frame(
        &self,
        id: u64,
        revision: u64,
        offset: u32,
        length: u32,
    ) -> Result<&[u8], GdiError32> {
        let surface = self
            .surfaces
            .iter()
            .find(|surface| {
                surface.id == id
                    && surface.revision == revision
                    && matches!(surface.key, SurfaceKey32::Window { .. })
            })
            .ok_or(GdiError32::InvalidRequest)?;
        if length > 4096 {
            return Err(GdiError32::InvalidRequest);
        }
        let Storage32::Owned(bytes) = &surface.storage else {
            return Err(GdiError32::InvalidRequest);
        };
        bytes
            .get(
                offset as usize
                    ..(u64::from(offset) + u64::from(length))
                        .try_into()
                        .map_err(|_| GdiError32::InvalidRequest)?,
            )
            .ok_or(GdiError32::InvalidRequest)
    }
    pub(in crate::process) fn acknowledge_frame(
        &mut self,
        id: u64,
        revision: u64,
    ) -> Result<(), GdiError32> {
        let surface = self
            .surfaces
            .iter_mut()
            .find(|surface| {
                surface.id == id
                    && surface.revision == revision
                    && matches!(surface.key, SurfaceKey32::Window { .. })
            })
            .ok_or(GdiError32::InvalidRequest)?;
        surface.damage = Rect32::default();
        Ok(())
    }
}
