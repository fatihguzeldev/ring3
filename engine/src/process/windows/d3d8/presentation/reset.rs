use super::super::{
    D3D8Runtime32, DispatchError32,
    garbage::Garbage32,
    lifetime::LifetimeChange32,
    memory::{check_output, read_words, word},
    objects::ObjectKind32,
};
use crate::{
    graphics::{
        ResourceId,
        d3d8::{
            api::{HResult, PresentationParameters32},
            commands::CommandError,
            device::{DeviceDescription, DeviceError},
            resources::Format,
        },
    },
    memory::{AddressSpace, GuestAddress},
    windows::window::{WindowContext32, WindowHandle32},
};

impl D3D8Runtime32 {
    pub(in crate::process::windows::d3d8) fn reset_device(
        &mut self,
        memory: &mut AddressSpace,
        windows: &WindowContext32,
        pointer: GuestAddress,
        parameters: u32,
    ) -> Result<(), DispatchError32> {
        check_output(memory, parameters, 52)?;
        let input = PresentationParameters32::decode(read_words(memory, parameters)?);
        let result = self.reset_validated(memory, windows, pointer, parameters, input);
        if let Err(error) = &result
            && !matches!(
                error,
                DispatchError32::Device(
                    DeviceError::Command(CommandError::Backpressure) | DeviceError::PendingReadback
                )
            )
            && let Ok(device) = self.device_mut(pointer)
        {
            device.reset_failed();
        }
        result
    }

    fn reset_validated(
        &mut self,
        memory: &mut AddressSpace,
        windows: &WindowContext32,
        pointer: GuestAddress,
        parameters: u32,
        input: PresentationParameters32,
    ) -> Result<(), DispatchError32> {
        let slot = self.record_slot(pointer)?;
        let ObjectKind32::Device(owner) = &self.records[slot].as_ref().unwrap().kind else {
            return Err(HResult::INVALID_CALL.into());
        };
        let focus = owner.focus_window;
        let behavior = owner.behavior;
        let window = WindowHandle32::try_from(if input.window == 0 {
            focus
        } else {
            input.window
        })
        .map_err(|_| HResult::INVALID_CALL)?;
        let extent = windows
            .get(window)
            .map_err(|_| HResult::INVALID_CALL)?
            .description
            .extent;
        let presentation = input.normalize(focus, extent, &self.config.adapter, false)?;
        if owner
            .implicit
            .iter()
            .any(|surface| self.com.references(*surface).is_ok_and(|count| count != 0))
        {
            return Err(DeviceError::Lost.into());
        }
        let mut unhold = Vec::new();
        unhold
            .try_reserve_exact(owner.implicit.len() + owner.resource_holds.len() + 2)
            .map_err(|_| HResult::OUT_OF_MEMORY)?;
        unhold.extend_from_slice(&owner.implicit);
        unhold.extend_from_slice(&owner.resource_holds);
        unhold.extend(owner.target_holds.iter().flatten().copied());
        let mut covered = Vec::new();
        covered
            .try_reserve_exact(owner.backend.resources().ids().count())
            .map_err(|_| HResult::OUT_OF_MEMORY)?;
        for index in 0..owner.backend.description().back_buffers {
            covered.push(owner.backend.back_buffer(index)?.resource);
        }
        covered.extend(
            owner
                .backend
                .depth_surface()
                .map(|surface| surface.resource),
        );
        let mut writes = Vec::new();
        writes
            .try_reserve_exact(13)
            .map_err(|_| HResult::OUT_OF_MEMORY)?;
        writes.extend(
            presentation
                .words()
                .into_iter()
                .enumerate()
                .map(|(index, value)| word(parameters + index as u32 * 4, value)),
        );
        let lifetime = self.prepare_lifetime(
            memory,
            LifetimeChange32 {
                unhold,
                ..Default::default()
            },
            &writes,
        )?;
        let mut disposal = Vec::new();
        disposal
            .try_reserve_exact(self.device(pointer)?.resources().ids().count())
            .map_err(|_| HResult::OUT_OF_MEMORY)?;
        for identity in lifetime.destroyed() {
            if let Some(ObjectKind32::Resource(resource)) = self.records[identity.payload as usize]
                .as_ref()
                .map(|object| &object.kind)
                && resource.device == pointer && !disposal.contains(&resource.resource)
            {
                disposal.push(resource.resource);
            }
        }
        for garbage in &self.garbage {
            if let Garbage32::Resource { device, resource } = *garbage
                && device == pointer
                    && !self.live_resource_identity(resource)
                    && !disposal.contains(&resource)
            {
                disposal.push(resource);
            }
        }
        if self.device(pointer)?.resources().ids().any(|id| {
            self.device(pointer)
                .unwrap()
                .resources()
                .desc(id)
                .is_ok_and(|desc| desc.pool() == crate::graphics::d3d8::resources::Pool::Default)
                && !covered.contains(&id)
                && !disposal.contains(&id)
        }) {
            return Err(DeviceError::Lost.into());
        }
        let description = DeviceDescription {
            width: presentation.width,
            height: presentation.height,
            format: Format::try_from(presentation.format).map_err(DeviceError::from)?,
            back_buffers: presentation.back_buffers,
            multisample: presentation.multisample,
            depth: if presentation.auto_depth {
                Some(Format::try_from(presentation.depth_format).map_err(DeviceError::from)?)
            } else {
                None
            },
            window: presentation.window,
            interval: presentation.interval,
            swap_effect: presentation.swap_effect,
            lockable_back_buffer: presentation.flags & 1 != 0,
            behavior,
        };
        let prepared = self
            .device(pointer)?
            .prepare_reset_with_disposal(description, &disposal)?;
        self.device(pointer)?.can_commit_reset(&prepared)?;
        for id in &disposal {
            if !covered.contains(id) {
                covered.push(*id);
            }
        }
        self.commit_lifetime(memory, lifetime);
        self.device_mut(pointer)
            .expect("reset device remains owned after COM state release")
            .commit_reset(prepared)
            .expect("exclusive reset preparation remains current after guest publication");
        let ObjectKind32::Device(owner) = &mut self.records[slot].as_mut().unwrap().kind else {
            unreachable!()
        };
        owner.presentation = presentation;
        owner.implicit.clear();
        owner.resource_holds.clear();
        owner.target_holds = [None; 2];
        self.garbage.retain(|garbage| match garbage {
            Garbage32::Resource { device, resource }
            | Garbage32::Namespace {
                device, resource, ..
            } => *device != pointer || !covered.contains(resource),
        });
        Ok(())
    }

    fn live_resource_identity(&self, id: ResourceId) -> bool {
        self.records.iter().flatten().any(|object|matches!(object.kind,ObjectKind32::Resource(resource) if resource.resource==id))
    }
}
