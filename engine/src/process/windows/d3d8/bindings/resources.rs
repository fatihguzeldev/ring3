use super::super::{D3D8Runtime32, DispatchError32, lifetime::LifetimeChange32, memory::word};
use crate::{
    graphics::d3d8::{
        api::{HResult, InterfaceKind},
        device::Binding,
        state::{TEXTURE_STAGES, VERTEX_STREAMS},
    },
    memory::{AddressSpace, GuestAddress},
};

impl D3D8Runtime32 {
    pub(in crate::process::windows::d3d8) fn resource_binding_call(
        &mut self,
        memory: &mut AddressSpace,
        pointer: GuestAddress,
        slot: u8,
        args: &[u32],
    ) -> Result<Option<u32>, DispatchError32> {
        match slot {
            61 | 83 | 85 => {
                let (raw, expected) = match slot {
                    61 => (args[2], InterfaceKind::BaseTexture),
                    83 => (args[2], InterfaceKind::VertexBuffer),
                    _ => (args[1], InterfaceKind::IndexBuffer),
                };
                let handle = if raw == 0 {
                    0
                } else {
                    let object = self.object(GuestAddress(raw))?;
                    if !object.interface.supports(expected) {
                        return Err(HResult::INVALID_CALL.into());
                    }
                    self.resource(GuestAddress(raw), pointer)?;
                    self.com.preflight(memory, GuestAddress(raw))?;
                    object.pointer.0
                };
                let binding = match slot {
                    61 => Binding::Texture {
                        stage: args[1],
                        handle,
                    },
                    83 => Binding::Stream {
                        stream: args[1],
                        handle,
                        stride: args[3],
                    },
                    _ => Binding::Indices {
                        handle,
                        base_vertex: args[2],
                    },
                };
                let prepared = self.device(pointer)?.prepare_binding(binding)?;
                let unchanged = self.binding_lifetime_unchanged(
                    memory,
                    pointer,
                    prepared.released_resource(),
                    prepared.retained_resource(),
                )?;
                if unchanged {
                    self.device(pointer)?.can_commit_binding(&prepared)?;
                } else {
                    let delta = self.binding_hold_delta(
                        pointer,
                        prepared.released_resource().into_iter(),
                        prepared.retained_resource().into_iter(),
                    )?;
                    self.device(pointer)?.can_commit_binding(&prepared)?;
                    self.commit_binding_holds(memory, pointer, delta, &[])?;
                }
                self.device_mut(pointer)?.commit_binding(prepared).expect("exclusive device binding preparation remains current after COM lifetime commit");
                if unchanged {
                    let slot = self
                        .record_slot(pointer)
                        .expect("committed binding device remains owned");
                    let super::super::objects::ObjectKind32::Device(device) =
                        &mut self.records[slot].as_mut().unwrap().kind
                    else {
                        unreachable!()
                    };
                    device.binding_lifetime_skips = device.binding_lifetime_skips.saturating_add(1);
                }
            }
            60 | 84 | 86 => {
                let state = self.device(pointer)?.state().snapshot();
                let (handle, output, extra) = match slot {
                    60 if args[1] < TEXTURE_STAGES as u32 => {
                        (state.textures[args[1] as usize], args[2], None)
                    }
                    84 if args[1] < VERTEX_STREAMS as u32 => {
                        let stream = state.streams[args[1] as usize];
                        (stream.resource, args[2], Some(word(args[3], stream.stride)))
                    }
                    86 => (
                        state.indices,
                        args[1],
                        Some(word(args[2], state.base_vertex)),
                    ),
                    _ => return Err(HResult::INVALID_CALL.into()),
                };
                let words = [word(output, handle), extra.unwrap_or(word(output, handle))];
                let writes = if extra.is_some() {
                    &words[..]
                } else {
                    &words[..1]
                };
                let mut retain = Vec::new();
                retain
                    .try_reserve_exact(1)
                    .map_err(|_| HResult::OUT_OF_MEMORY)?;
                if handle != 0 {
                    retain.push(self.counter_pointer(GuestAddress(handle))?);
                }
                self.change_lifetime(
                    memory,
                    LifetimeChange32 {
                        retain,
                        ..Default::default()
                    },
                    writes,
                )?;
            }
            _ => return Ok(None),
        }
        Ok(Some(0))
    }
}
