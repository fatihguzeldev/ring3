use super::{
    super::{D3D8Runtime32, DispatchError32, memory::read_words, objects::ObjectKind32},
    error,
};
use crate::{
    graphics::d3d8::{
        api::InterfaceKind,
        resources::{Box3, LockFlags, ResourceDesc, SurfaceRef},
    },
    memory::{AddressSpace, GuestAddress},
};

impl D3D8Runtime32 {
    pub(in crate::process::windows::d3d8) fn resource_call(
        &mut self,
        memory: &mut AddressSpace,
        pointer: GuestAddress,
        interface: InterfaceKind,
        slot: u8,
        args: &[u32],
    ) -> Result<Option<u32>, DispatchError32> {
        let (device, surface) = match self.object(pointer)?.kind {
            ObjectKind32::Resource(resource) => (
                resource.device,
                SurfaceRef {
                    resource: resource.resource,
                    subresource: 0,
                },
            ),
            ObjectKind32::View(view) => (view.device, view.surface),
            _ => return Ok(None),
        };
        if slot == 3 {
            self.acquire_output(memory, device, args[1])?;
            return Ok(Some(0));
        }
        if (4..=6).contains(&slot) {
            return self
                .private_resource_call(memory, pointer, device, surface.resource, slot, args)
                .map(Some);
        }
        let image = matches!(interface, InterfaceKind::Surface | InterfaceKind::Volume);
        if image {
            return match slot {
                7 => self
                    .resource_container(memory, pointer, args[1], args[2])
                    .map(Some),
                8 => {
                    self.write_resource_description(memory, device, surface, interface, args[1])?;
                    Ok(Some(0))
                }
                9 => {
                    let region = read_region(memory, args[2], interface == InterfaceKind::Volume)?;
                    self.lock_image(
                        memory,
                        device,
                        surface,
                        region,
                        LockFlags(args[3]),
                        args[1],
                        interface == InterfaceKind::Volume,
                    )?;
                    Ok(Some(0))
                }
                10 => {
                    self.unlock_guest(memory, device, surface)?;
                    Ok(Some(0))
                }
                _ => Ok(None),
            };
        }
        let resource = surface.resource;
        let common = match slot {
            7 => Some(
                self.device_mut(device)?
                    .resources_mut()?
                    .set_priority(resource, args[1])
                    .map_err(error)?,
            ),
            8 => Some(
                self.device(device)?
                    .resources()
                    .priority(resource)
                    .map_err(error)?,
            ),
            9 => {
                self.preload_resource(device, resource)?;
                Some(0)
            }
            10 => Some(
                self.device(device)?
                    .resources()
                    .desc(resource)
                    .map_err(error)?
                    .resource_type(),
            ),
            _ => None,
        };
        if common.is_some() {
            return Ok(common);
        }
        if matches!(
            interface,
            InterfaceKind::VertexBuffer | InterfaceKind::IndexBuffer
        ) {
            return match slot {
                11 => {
                    self.lock_buffer_guest(
                        memory,
                        device,
                        resource,
                        args[1],
                        args[2],
                        LockFlags(args[4]),
                        args[3],
                    )?;
                    Ok(Some(0))
                }
                12 => {
                    self.unlock_guest(memory, device, surface)?;
                    Ok(Some(0))
                }
                13 => {
                    self.write_resource_description(memory, device, surface, interface, args[1])?;
                    Ok(Some(0))
                }
                _ => Ok(None),
            };
        }
        let ResourceDesc::Texture(desc) = self
            .device(device)?
            .resources()
            .desc(resource)
            .map_err(error)?
        else {
            return Ok(None);
        };
        match slot {
            11 => Ok(Some(
                self.device_mut(device)?
                    .resources_mut()?
                    .set_lod(resource, args[1])
                    .map_err(error)?,
            )),
            12 => Ok(Some(
                self.device(device)?
                    .resources()
                    .lod(resource)
                    .map_err(error)?,
            )),
            13 => Ok(Some(desc.levels)),
            14 => {
                let mip = self.mip_surface(device, resource, 0, args[1])?;
                self.write_resource_description(memory, device, mip, interface, args[2])?;
                Ok(Some(0))
            }
            15 => {
                let cube = interface == InterfaceKind::CubeTexture;
                let mip = self.mip_surface(
                    device,
                    resource,
                    if cube { args[1] } else { 0 },
                    if cube { args[2] } else { args[1] },
                )?;
                self.acquire_resource_view(
                    memory,
                    pointer,
                    device,
                    mip,
                    if interface == InterfaceKind::VolumeTexture {
                        InterfaceKind::Volume
                    } else {
                        InterfaceKind::Surface
                    },
                    if cube { args[3] } else { args[2] },
                )?;
                Ok(Some(0))
            }
            16 => {
                let cube = interface == InterfaceKind::CubeTexture;
                let mip = self.mip_surface(
                    device,
                    resource,
                    if cube { args[1] } else { 0 },
                    if cube { args[2] } else { args[1] },
                )?;
                let output_index = if cube { 3 } else { 2 };
                let region = read_region(
                    memory,
                    args[output_index + 1],
                    interface == InterfaceKind::VolumeTexture,
                )?;
                self.lock_image(
                    memory,
                    device,
                    mip,
                    region,
                    LockFlags(args[output_index + 2]),
                    args[output_index],
                    interface == InterfaceKind::VolumeTexture,
                )?;
                Ok(Some(0))
            }
            17 => {
                let cube = interface == InterfaceKind::CubeTexture;
                let mip = self.mip_surface(
                    device,
                    resource,
                    if cube { args[1] } else { 0 },
                    if cube { args[2] } else { args[1] },
                )?;
                self.unlock_guest(memory, device, mip)?;
                Ok(Some(0))
            }
            18 => {
                let cube = interface == InterfaceKind::CubeTexture;
                let region = read_region(
                    memory,
                    if cube { args[2] } else { args[1] },
                    interface == InterfaceKind::VolumeTexture,
                )?;
                self.device_mut(device)?
                    .resources_mut()?
                    .add_dirty(resource, if cube { args[1] } else { 0 }, region)
                    .map_err(error)?;
                Ok(Some(0))
            }
            _ => Ok(None),
        }
    }
}

pub(super) fn read_region(
    memory: &AddressSpace,
    pointer: u32,
    volume: bool,
) -> Result<Option<Box3>, DispatchError32> {
    if pointer == 0 {
        return Ok(None);
    }
    Ok(Some(if volume {
        let [left, top, right, bottom, front, back] = read_words(memory, pointer)?;
        Box3 {
            left,
            top,
            front,
            right,
            bottom,
            back,
        }
    } else {
        let [left, top, right, bottom] = read_words(memory, pointer)?;
        Box3 {
            left,
            top,
            front: 0,
            right,
            bottom,
            back: 1,
        }
    }))
}
