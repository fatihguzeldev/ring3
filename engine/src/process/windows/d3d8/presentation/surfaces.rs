use super::super::{
    D3D8Runtime32, DispatchError32,
    lifetime::LifetimeChange32,
    memory::check_output,
    objects::{Object32, ObjectKind32, Resource32},
};
use crate::{
    graphics::d3d8::{
        api::{HResult, InterfaceKind},
        resources::{ResourceDesc, SurfaceRef},
    },
    memory::{AddressSpace, GuestAddress},
    process::windows::objects::{GuestComCreation32, InterfaceLayout32},
    windows::com::ReferenceState32,
};

impl D3D8Runtime32 {
    pub(in crate::process::windows::d3d8) fn acquire_surface(
        &mut self,
        memory: &mut AddressSpace,
        device: GuestAddress,
        owner: GuestAddress,
        surface: SurfaceRef,
        output: u32,
    ) -> Result<(), DispatchError32> {
        check_output(memory, output, 4)?;
        if let Some(pointer) = self
            .records
            .iter()
            .flatten()
            .find_map(|object| match object.kind {
                ObjectKind32::Resource(resource)
                    if resource.device == device
                        && resource.resource == surface.resource
                        && surface.subresource == 0
                        && object.interface == InterfaceKind::Surface =>
                {
                    Some(object.pointer)
                }
                ObjectKind32::View(view) if view.device == device && view.surface == surface => {
                    Some(object.pointer)
                }
                _ => None,
            })
        {
            self.acquire_output(memory, pointer, output)?;
            return Ok(());
        }
        if matches!(
            self.device(device)?
                .resources()
                .desc(surface.resource)
                .map_err(crate::graphics::d3d8::device::DeviceError::from)?,
            ResourceDesc::Texture(_)
        ) {
            let container = self
                .records
                .iter()
                .flatten()
                .find_map(|object| match object.kind {
                    ObjectKind32::Resource(resource)
                        if resource.device == device && resource.resource == surface.resource =>
                    {
                        Some(object.pointer)
                    }
                    _ => None,
                })
                .ok_or(HResult::INVALID_CALL)?;
            return self.acquire_resource_view(
                memory,
                container,
                device,
                surface,
                InterfaceKind::Surface,
                output,
            );
        }
        let owner = self
            .records
            .iter()
            .flatten()
            .find_map(|object| match &object.kind {
                ObjectKind32::SwapChain(chain)
                    if chain.device == device && chain.resources.contains(&surface) =>
                {
                    Some(object.pointer)
                }
                _ => None,
            })
            .unwrap_or(owner);
        let owner_slot = self.record_slot(owner)?;
        let surfaces = match &self.records[owner_slot].as_ref().unwrap().kind {
            ObjectKind32::Device(device) => &device.implicit,
            ObjectKind32::SwapChain(chain) => &chain.surfaces,
            _ => return Err(HResult::INVALID_CALL.into()),
        };
        if surfaces.len() == surfaces.capacity() {
            return Err(HResult::OUT_OF_MEMORY.into());
        }
        let slot = self.vacant_record()?;
        let identity = self.com.create_with_lifetime(
            memory,
            slot as u64,
            &[InterfaceLayout32 {
                iid: InterfaceKind::Surface.iid(),
                methods: &self.methods[InterfaceKind::Surface as usize - 1],
            }],
            GuestComCreation32 {
                output: Some((GuestAddress(output), InterfaceKind::Surface.iid())),
                writes: &[],
                initial: ReferenceState32 {
                    references: 1,
                    holds: 1,
                },
                retain: &[device],
            },
        )?;
        let pointer = identity.identity_pointer;
        self.records[slot] = Some(Object32 {
            pointer,
            interface: InterfaceKind::Surface,
            private_unknowns: Vec::new(),
            kind: ObjectKind32::Resource(Resource32 {
                device,
                resource: surface.resource,
            }),
        });
        match &mut self.records[owner_slot].as_mut().unwrap().kind {
            ObjectKind32::Device(device) => device.implicit.push(pointer),
            ObjectKind32::SwapChain(chain) => chain.surfaces.push(pointer),
            _ => unreachable!(),
        }
        Ok(())
    }

    pub(in crate::process::windows::d3d8) fn set_targets_guest(
        &mut self,
        memory: &mut AddressSpace,
        pointer: GuestAddress,
        color: u32,
        depth: u32,
    ) -> Result<(), DispatchError32> {
        let (color_surface, color_counter) = self.target_surface(memory, pointer, color)?;
        let (depth_surface, depth_counter) = self.target_surface(memory, pointer, depth)?;
        let prepared = self
            .device(pointer)?
            .prepare_targets(color_surface, depth_surface)?;
        let slot = self.record_slot(pointer)?;
        let ObjectKind32::Device(device) = &self.records[slot].as_ref().unwrap().kind else {
            return Err(HResult::INVALID_CALL.into());
        };
        let old = device.target_holds;
        let next = [
            if color == 0 { old[0] } else { color_counter },
            depth_counter,
        ];
        let mut hold = Vec::new();
        hold.try_reserve_exact(2)
            .map_err(|_| HResult::OUT_OF_MEMORY)?;
        hold.extend(next.iter().flatten().copied());
        let mut unhold = Vec::new();
        unhold
            .try_reserve_exact(2)
            .map_err(|_| HResult::OUT_OF_MEMORY)?;
        unhold.extend(old.iter().flatten().copied());
        let lifetime = self.prepare_lifetime(
            memory,
            LifetimeChange32 {
                hold,
                unhold,
                ..Default::default()
            },
            &[],
        )?;
        self.device(pointer)?.can_commit_targets(&prepared)?;
        self.commit_lifetime(memory, lifetime);
        self.device_mut(pointer)
            .expect("active device remains owned during target publication")
            .commit_targets(prepared)
            .expect("exclusive target preparation remains current after COM lifetime commit");
        let ObjectKind32::Device(device) = &mut self.records[slot].as_mut().unwrap().kind else {
            unreachable!()
        };
        device.target_holds = next;
        Ok(())
    }

    fn target_surface(
        &self,
        memory: &AddressSpace,
        device: GuestAddress,
        pointer: u32,
    ) -> Result<(Option<SurfaceRef>, Option<GuestAddress>), DispatchError32> {
        if pointer == 0 {
            return Ok((None, None));
        }
        let pointer = GuestAddress(pointer);
        if self.object(pointer)?.interface != InterfaceKind::Surface {
            return Err(HResult::INVALID_CALL.into());
        }
        self.com.preflight(memory, pointer)?;
        Ok((
            Some(self.resource(pointer, device)?),
            Some(self.counter_pointer(pointer)?),
        ))
    }
}
