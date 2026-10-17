use super::types::*;
use crate::{
    memory::AddressSpace,
    process::windows::gdi::{
        context::GdiRuntime32,
        storage::{Storage32, SurfaceKey32},
    },
    windows::gdi::{GdiError32, GdiImageObject32},
};

#[derive(Clone, Copy)]
pub(in crate::process::windows::gdi) struct ImageOwner32 {
    pub handle: u32,
    pub object: GdiImageObject32,
    pub mask: (u64, u64),
    pub color: Option<(u64, u64)>,
}
pub(in crate::process) struct PreparedGdiImageDelete32 {
    pub(super) memory: u64,
    pub(super) owner: ImageOwner32,
}
impl GdiRuntime32 {
    pub(in crate::process::windows::gdi) fn image_owner(
        &self,
        handle: u32,
    ) -> Result<ImageOwner32, GdiError32> {
        let object = self.model.image_object(handle)?;
        let mask = &self.surfaces[self.surface_index(SurfaceKey32::ImageMask(handle))?];
        let color = if object.monochrome {
            None
        } else {
            let color = &self.surfaces[self.surface_index(SurfaceKey32::ImageColor(handle))?];
            Some((color.id, color.revision))
        };
        Ok(ImageOwner32 {
            handle,
            object,
            mask: (mask.id, mask.revision),
            color,
        })
    }
    pub(in crate::process::windows::gdi) fn check_image_owner(
        &self,
        owner: ImageOwner32,
    ) -> Result<(), GdiError32> {
        let current = self.image_owner(owner.handle)?;
        if current.object != owner.object
            || current.mask != owner.mask
            || current.color != owner.color
        {
            return Err(GdiError32::InvalidRequest);
        }
        Ok(())
    }
    pub(super) fn image_retirement_size(
        &self,
        owner: Option<ImageOwner32>,
    ) -> Result<(usize, usize), GdiError32> {
        let Some(owner) = owner else {
            return Ok((0, 0));
        };
        self.check_image_owner(owner)?;
        let mask = &self.surfaces[self.surface_index(SurfaceKey32::ImageMask(owner.handle))?];
        let color = owner
            .color
            .map(|_| self.surface_index(SurfaceKey32::ImageColor(owner.handle)))
            .transpose()?
            .map(|index| self.surfaces[index].layout.byte_len())
            .transpose()?
            .unwrap_or(0);
        Ok((
            1 + usize::from(owner.color.is_some()),
            mask.layout.byte_len()? + color,
        ))
    }
    pub(super) fn retire_image_surfaces(&mut self, owner: ImageOwner32) {
        for key in [
            Some(SurfaceKey32::ImageMask(owner.handle)),
            owner.color.map(|_| SurfaceKey32::ImageColor(owner.handle)),
        ]
        .into_iter()
        .flatten()
        {
            let index = self.surface_index(key).expect("prepared image plane");
            let surface = self.surfaces.remove(index);
            self.bytes -= surface.layout.byte_len().expect("published image size");
        }
    }
    pub(in crate::process) fn image_info(
        &self,
        handle: u32,
    ) -> Result<GdiImageInfo32<'_>, GdiError32> {
        let owner = self.image_owner(handle)?;
        let view = |key| -> Result<GdiBitmapView32<'_>, GdiError32> {
            let surface = &self.surfaces[self.surface_index(key)?];
            let Storage32::Owned(pixels) = &surface.storage else {
                return Err(GdiError32::InvalidRequest);
            };
            Ok(GdiBitmapView32 {
                layout: surface.layout,
                palette: &surface.palette,
                pixels,
            })
        };
        Ok(GdiImageInfo32 {
            kind: owner.object.kind,
            width: owner.object.width,
            height: owner.object.height,
            monochrome: owner.object.monochrome,
            hotspot: owner.object.hotspot,
            alpha: owner.object.alpha,
            mask: view(SurfaceKey32::ImageMask(handle))?,
            color: owner
                .color
                .map(|_| view(SurfaceKey32::ImageColor(handle)))
                .transpose()?,
        })
    }
    pub(in crate::process) fn prepare_delete_image(
        &self,
        memory: &AddressSpace,
        handle: u32,
    ) -> Result<PreparedGdiImageDelete32, GdiError32> {
        Ok(PreparedGdiImageDelete32 {
            memory: memory.identity(),
            owner: self.image_owner(handle)?,
        })
    }
    pub(in crate::process) fn check_delete_image(
        &self,
        memory: &AddressSpace,
        receipt: &PreparedGdiImageDelete32,
    ) -> Result<(), GdiError32> {
        if memory.identity() != receipt.memory {
            return Err(GdiError32::InvalidRequest);
        }
        self.check_image_owner(receipt.owner)
    }
    pub(in crate::process) fn commit_delete_image(
        &mut self,
        memory: &mut AddressSpace,
        receipt: PreparedGdiImageDelete32,
    ) -> Result<(), GdiError32> {
        self.check_delete_image(memory, &receipt)?;
        self.retire_image_surfaces(receipt.owner);
        self.model
            .delete_image_object(receipt.owner.handle)
            .expect("checked image identity");
        Ok(())
    }
}
