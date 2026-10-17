use super::{
    ownership::ImageOwner32,
    planes::{ImagePlane32, ImagePlanes32},
    types::*,
};
use crate::{
    memory::AddressSpace,
    process::windows::gdi::{
        context::GdiRuntime32,
        storage::{Storage32, Surface32, SurfaceKey32},
    },
    windows::gdi::{GdiError32, GdiImageObject32, GdiObject32, PreparedGdiObject32},
};
pub(in crate::process) struct PreparedGdiImage32 {
    memory: u64,
    first_surface: u64,
    object: PreparedGdiObject32,
    mask: Surface32,
    color: Option<Surface32>,
    retire: Option<ImageOwner32>,
    source: Option<ImageOwner32>,
}

impl GdiRuntime32 {
    pub(in crate::process) fn prepare_image_group(
        &self,
        memory: &mut AddressSpace,
        request: GdiImageRequest32<'_>,
        replace: Option<u32>,
    ) -> Result<PreparedGdiImage32, GdiError32> {
        let planes = self.decode_image_planes(request)?;
        self.prepare_image_planes(memory, request.kind, planes, replace, None)
    }
    pub(super) fn prepare_image_planes(
        &self,
        memory: &AddressSpace,
        kind: GdiImageKind32,
        planes: ImagePlanes32,
        replace: Option<u32>,
        source: Option<ImageOwner32>,
    ) -> Result<PreparedGdiImage32, GdiError32> {
        let metadata = GdiImageObject32 {
            kind,
            width: planes.width,
            height: planes.height,
            hotspot: planes.hotspot,
            monochrome: planes.color.is_none(),
            alpha: planes.alpha,
        };
        let retire = replace.map(|handle| self.image_owner(handle)).transpose()?;
        if retire.is_some_and(|owner| owner.object.kind != kind) {
            return Err(GdiError32::InvalidHandle);
        }
        let object = self
            .model
            .prepare_object_replacing(GdiObject32::Image(metadata), replace)?;
        let count = 1 + usize::from(planes.color.is_some());
        let size = planes
            .mask
            .layout
            .byte_len()?
            .checked_add(
                planes
                    .color
                    .as_ref()
                    .map(|plane| plane.layout.byte_len())
                    .transpose()?
                    .unwrap_or(0),
            )
            .ok_or(GdiError32::Capacity)?;
        self.check_image_capacity(retire, count, size)?;
        self.next_surface
            .checked_add(count as u64)
            .ok_or(GdiError32::Exhausted)?;
        let make = |plane: ImagePlane32, key, id| Surface32 {
            key,
            id,
            revision: 1,
            damage: Default::default(),
            layout: plane.layout,
            palette: plane.colors,
            storage: Storage32::Owned(plane.pixels),
        };
        let mask = make(
            planes.mask,
            SurfaceKey32::ImageMask(object.handle()),
            self.next_surface,
        );
        let color = planes.color.map(|plane| {
            make(
                plane,
                SurfaceKey32::ImageColor(object.handle()),
                self.next_surface + 1,
            )
        });
        Ok(PreparedGdiImage32 {
            memory: memory.identity(),
            first_surface: self.next_surface,
            object,
            mask,
            color,
            retire,
            source,
        })
    }
    fn check_image_capacity(
        &self,
        retire: Option<ImageOwner32>,
        count: usize,
        size: usize,
    ) -> Result<(), GdiError32> {
        let (old_count, old_size) = self.image_retirement_size(retire)?;
        if self.surfaces.len() - old_count + count > self.config.objects + self.config.dcs
            || size
                > self
                    .config
                    .bitmap_bytes
                    .saturating_sub(self.bytes - old_size + self.model.palette_bytes())
        {
            return Err(GdiError32::Capacity);
        }
        Ok(())
    }
    pub(in crate::process) fn check_image_group(
        &self,
        memory: &AddressSpace,
        receipt: &PreparedGdiImage32,
    ) -> Result<(), GdiError32> {
        if memory.identity() != receipt.memory || self.next_surface != receipt.first_surface {
            return Err(GdiError32::InvalidRequest);
        }
        self.model.check_prepared_new(&receipt.object)?;
        if let Some(source) = receipt.source {
            self.check_image_owner(source)?;
        }
        let count = 1 + usize::from(receipt.color.is_some());
        let size = receipt.mask.layout.byte_len()?
            + receipt
                .color
                .as_ref()
                .map(|plane| plane.layout.byte_len())
                .transpose()?
                .unwrap_or(0);
        self.check_image_capacity(receipt.retire, count, size)
    }
    pub(in crate::process) fn commit_image_group(
        &mut self,
        memory: &mut AddressSpace,
        receipt: PreparedGdiImage32,
    ) -> Result<u32, GdiError32> {
        self.check_image_group(memory, &receipt)?;
        if let Some(retire) = receipt.retire {
            self.retire_image_surfaces(retire);
        }
        let handle = self.model.commit_new(receipt.object);
        self.publish_surface(receipt.mask);
        if let Some(color) = receipt.color {
            self.publish_surface(color);
        }
        Ok(handle)
    }
}
