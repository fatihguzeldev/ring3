use super::super::memory::read_bytes;
use super::super::{D3D8Runtime32, DispatchError32};
use super::decoding::{clear_rectangles, vertex_bytes};
use crate::{
    graphics::d3d8::{
        api::HResult,
        commands::{
            DrawBindings, DrawData, IndexBinding, Primitive, ResourceVersion, StreamResource,
            TextureBinding,
        },
        device::DeviceError,
        resources::{BufferKind, Format, ResourceDesc},
    },
    memory::{AddressSpace, GuestAddress},
};

impl D3D8Runtime32 {
    pub(in crate::process::windows::d3d8) fn draw_call(
        &mut self,
        memory: &mut AddressSpace,
        pointer: GuestAddress,
        slot: u8,
        args: &[u32],
    ) -> Result<Option<u32>, DispatchError32> {
        match slot {
            34 => self.device_mut(pointer)?.begin_scene()?,
            35 => self.device_mut(pointer)?.end_scene()?,
            36 => {
                let device = self.device(pointer)?;
                let viewport = device.state().snapshot().viewport;
                let rectangles = clear_rectangles(
                    memory,
                    args[2],
                    args[1],
                    viewport,
                    device.queue().limits().pending_bytes,
                )?;
                if args[1] != 0 && rectangles.is_empty() {
                    device.validate_clear(args[3], args[5])?;
                } else {
                    self.device_mut(pointer)?.clear(
                        args[3],
                        args[4],
                        args[5],
                        args[6],
                        &rectangles,
                    )?;
                }
            }
            70 => {
                let primitive = Primitive::try_from(args[1]).map_err(|_| HResult::INVALID_CALL)?;
                let bindings = self.draw_bindings(pointer, true, false)?;
                self.device_mut(pointer)?.draw(
                    primitive,
                    args[3],
                    bindings,
                    DrawData::Vertices {
                        first_vertex: args[2],
                    },
                )?;
            }
            71 => {
                let primitive = Primitive::try_from(args[1]).map_err(|_| HResult::INVALID_CALL)?;
                let bindings = self.draw_bindings(pointer, true, true)?;
                self.device_mut(pointer)?.draw(
                    primitive,
                    args[5],
                    bindings,
                    DrawData::Indexed {
                        min_vertex: args[2],
                        vertex_count: args[3],
                        start_index: args[4],
                    },
                )?;
            }
            72 => {
                let primitive = Primitive::try_from(args[1]).map_err(|_| HResult::INVALID_CALL)?;
                let count = primitive
                    .element_count(args[2])
                    .ok_or(HResult::INVALID_CALL)?;
                let maximum = self.device(pointer)?.queue().limits().pending_bytes;
                let vertices = vertex_bytes(memory, args[3], 0, count, args[4], maximum)?;
                let bindings = self.draw_bindings(pointer, false, false)?;
                let holds = self.preflight_up_holds(memory, pointer, false)?;
                self.device_mut(pointer)?.draw(
                    primitive,
                    args[2],
                    bindings,
                    DrawData::UserVertices {
                        stride: args[4],
                        vertices: &vertices,
                    },
                )?;
                self.complete_up_holds(memory, pointer, holds);
            }
            73 => {
                let primitive = Primitive::try_from(args[1]).map_err(|_| HResult::INVALID_CALL)?;
                let count = primitive
                    .element_count(args[4])
                    .ok_or(HResult::INVALID_CALL)?;
                let format = Format::try_from(args[6]).map_err(DeviceError::from)?;
                let index_stride = match format {
                    Format::Index16 => 2,
                    Format::Index32 => 4,
                    _ => return Err(HResult::INVALID_CALL.into()),
                };
                let index_bytes = count
                    .checked_mul(index_stride)
                    .ok_or(HResult::INVALID_CALL)?;
                let maximum = self.device(pointer)?.queue().limits().pending_bytes;
                let vertex_length = args[3].checked_mul(args[8]).ok_or(HResult::INVALID_CALL)?;
                if index_bytes
                    .checked_add(vertex_length)
                    .is_none_or(|length| length > maximum)
                {
                    return Err(HResult::INVALID_CALL.into());
                }
                let vertices = vertex_bytes(memory, args[7], args[2], args[3], args[8], maximum)?;
                let indices = read_bytes(memory, args[5], index_bytes as usize)?;
                let bindings = self.draw_bindings(pointer, false, false)?;
                let holds = self.preflight_up_holds(memory, pointer, true)?;
                self.device_mut(pointer)?.draw(
                    primitive,
                    args[4],
                    bindings,
                    DrawData::UserIndexed {
                        min_vertex: args[2],
                        vertex_count: args[3],
                        stride: args[8],
                        vertices: &vertices,
                        format,
                        indices: &indices,
                    },
                )?;
                self.complete_up_holds(memory, pointer, holds);
            }
            74 => {
                let destination = self.resource(GuestAddress(args[4]), pointer)?;
                let bindings = self.draw_bindings(pointer, true, false)?;
                let outcome = self.device_mut(pointer)?.process_vertices(
                    args[1],
                    args[2],
                    args[3],
                    destination.resource,
                    args[5],
                    bindings,
                )?;
                if let Some(id) = outcome.value {
                    return Err(DispatchError32::Readback { id });
                }
            }
            94 | 95 => self.device(pointer)?.draw_patch()?,
            96 => self.device(pointer)?.delete_patch(args[1])?,
            _ => return Ok(None),
        }
        Ok(Some(0))
    }

    pub(in crate::process::windows::d3d8) fn draw_bindings(
        &self,
        pointer: GuestAddress,
        streams: bool,
        indexed: bool,
    ) -> Result<DrawBindings, DispatchError32> {
        let device = self.device(pointer)?;
        let state = device.state().snapshot();
        let mut bindings = DrawBindings::default();
        for (index, handle) in state.textures.iter().enumerate() {
            if *handle != 0 {
                let resource = self.binding_version(pointer, *handle)?;
                let lod = device
                    .resources()
                    .lod(resource.id)
                    .map_err(DeviceError::from)?;
                bindings.textures[index] = Some(TextureBinding { resource, lod });
            }
        }
        if streams {
            for (index, stream) in state.streams.iter().enumerate() {
                if stream.resource != 0 {
                    bindings.streams[index] = Some(StreamResource {
                        resource: self.binding_version(pointer, stream.resource)?,
                        stride: stream.stride,
                    });
                }
            }
        }
        if indexed {
            if state.indices == 0 {
                return Err(HResult::INVALID_CALL.into());
            }
            let resource = self.binding_version(pointer, state.indices)?;
            let ResourceDesc::Buffer(desc) = device
                .resources()
                .desc(resource.id)
                .map_err(DeviceError::from)?
            else {
                return Err(HResult::INVALID_CALL.into());
            };
            let BufferKind::Index { format } = desc.kind else {
                return Err(HResult::INVALID_CALL.into());
            };
            bindings.indices = Some(IndexBinding {
                resource,
                format,
                base_vertex: state.base_vertex,
            });
        }
        Ok(bindings)
    }

    fn binding_version(
        &self,
        device: GuestAddress,
        handle: u32,
    ) -> Result<ResourceVersion, DispatchError32> {
        let surface = self.resource(GuestAddress(handle), device)?;
        let version = self
            .device(device)?
            .resources()
            .version(surface.resource)
            .map_err(DeviceError::from)?;
        Ok(ResourceVersion {
            id: surface.resource,
            version,
        })
    }
}
