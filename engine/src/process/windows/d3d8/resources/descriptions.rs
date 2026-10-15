use super::{
    super::{D3D8Runtime32, DispatchError32, memory::write_words},
    error,
};
use crate::{
    graphics::d3d8::{
        api::{HResult, InterfaceKind},
        resources::{BufferKind, ResourceDesc, SurfaceRef},
    },
    memory::{AddressSpace, GuestAddress},
};

impl D3D8Runtime32 {
    pub(super) fn write_resource_description(
        &self,
        memory: &mut AddressSpace,
        device: GuestAddress,
        surface: SurfaceRef,
        interface: InterfaceKind,
        output: u32,
    ) -> Result<(), DispatchError32> {
        let resources = self.device(device)?.resources();
        let desc = resources.desc(surface.resource).map_err(error)?;
        if let ResourceDesc::Buffer(buffer) = desc {
            let mut words = [
                desc.format() as u32,
                desc.resource_type(),
                buffer.usage.0,
                buffer.pool as u32,
                buffer.length,
                0,
            ];
            let count = if let BufferKind::Vertex { fvf } = buffer.kind {
                words[5] = fvf;
                6
            } else {
                5
            };
            write_words(memory, output, &words[..count])?;
        } else {
            let info = resources.info(surface).map_err(error)?;
            let volume =
                interface == InterfaceKind::Volume || interface == InterfaceKind::VolumeTexture;
            if volume {
                write_words(
                    memory,
                    output,
                    &[
                        desc.format() as u32,
                        2,
                        desc.usage().0,
                        desc.pool() as u32,
                        info.length,
                        info.width,
                        info.height,
                        info.depth,
                    ],
                )?;
            } else {
                let multisample = match desc {
                    ResourceDesc::Surface(surface) => surface.multisample,
                    _ => 0,
                };
                write_words(
                    memory,
                    output,
                    &[
                        desc.format() as u32,
                        1,
                        desc.usage().0,
                        desc.pool() as u32,
                        info.length,
                        multisample,
                        info.width,
                        info.height,
                    ],
                )?;
            }
        }
        Ok(())
    }

    pub(super) fn mip_surface(
        &self,
        device: GuestAddress,
        resource: crate::graphics::ResourceId,
        face: u32,
        level: u32,
    ) -> Result<SurfaceRef, DispatchError32> {
        let ResourceDesc::Texture(desc) = self
            .device(device)?
            .resources()
            .desc(resource)
            .map_err(error)?
        else {
            return Err(HResult::INVALID_CALL.into());
        };
        let faces = if desc.kind == crate::graphics::d3d8::resources::TextureKind::Cube {
            6
        } else {
            1
        };
        if face >= faces || level >= desc.levels {
            return Err(HResult::INVALID_CALL.into());
        }
        Ok(SurfaceRef {
            resource,
            subresource: face * desc.levels + level,
        })
    }
}
