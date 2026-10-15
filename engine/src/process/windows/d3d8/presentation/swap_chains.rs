use super::super::{
    D3D8Runtime32, DispatchError32,
    memory::{check_output, read_words, word},
    objects::{Object32, ObjectKind32, SwapChain32},
};
use crate::{
    graphics::d3d8::{
        api::{HResult, InterfaceKind, PresentationParameters32},
        device::DeviceError,
        resources::{Format, Pool, ResourceDesc, SurfaceDesc, SurfaceRef, Usage},
    },
    memory::{AddressSpace, GuestAddress},
    process::windows::objects::{GuestComCreation32, InterfaceLayout32},
    windows::{
        com::ReferenceState32,
        window::{WindowContext32, WindowHandle32},
    },
};

impl D3D8Runtime32 {
    pub(in crate::process::windows::d3d8) fn create_swap_chain(
        &mut self,
        memory: &mut AddressSpace,
        windows: &WindowContext32,
        device: GuestAddress,
        parameters: u32,
        output: u32,
    ) -> Result<(), DispatchError32> {
        check_output(memory, output, 4)?;
        check_output(memory, parameters, 52)?;
        let input = PresentationParameters32::decode(read_words(memory, parameters)?);
        let slot = self.record_slot(device)?;
        let ObjectKind32::Device(owner) = &self.records[slot].as_ref().unwrap().kind else {
            return Err(HResult::INVALID_CALL.into());
        };
        let focus = owner.focus_window;
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
        let presentation = input.normalize(focus, extent, &self.config.adapter, true)?;
        let mut descriptions = Vec::new();
        descriptions
            .try_reserve_exact(presentation.back_buffers as usize)
            .map_err(|_| HResult::OUT_OF_MEMORY)?;
        descriptions.resize(
            presentation.back_buffers as usize,
            back_buffer_description(presentation)?,
        );
        let prepared = self.device(device)?.prepare_resources(&descriptions)?;
        let mut resources = Vec::new();
        resources
            .try_reserve_exact(prepared.ids().len())
            .map_err(|_| HResult::OUT_OF_MEMORY)?;
        resources.extend(prepared.ids().iter().map(|resource| SurfaceRef {
            resource: *resource,
            subresource: 0,
        }));
        let mut surfaces = Vec::new();
        surfaces
            .try_reserve_exact(resources.len())
            .map_err(|_| HResult::OUT_OF_MEMORY)?;
        let record = self.vacant_record()?;
        let words = presentation.words();
        let writes: Vec<_> = words
            .into_iter()
            .enumerate()
            .map(|(index, value)| word(parameters + index as u32 * 4, value))
            .collect();
        self.device(device)?.can_commit_resources(&prepared)?;
        let identity = self.com.create_with_lifetime(
            memory,
            record as u64,
            &[InterfaceLayout32 {
                iid: InterfaceKind::SwapChain.iid(),
                methods: &self.methods[InterfaceKind::SwapChain as usize - 1],
            }],
            GuestComCreation32 {
                output: Some((GuestAddress(output), InterfaceKind::SwapChain.iid())),
                writes: &writes,
                initial: ReferenceState32 {
                    references: 1,
                    holds: 0,
                },
                retain: &[device],
            },
        )?;
        self.records[record] = Some(Object32 {
            pointer: identity.identity_pointer,
            interface: InterfaceKind::SwapChain,
            private_unknowns: Vec::new(),
            kind: ObjectKind32::SwapChain(SwapChain32 {
                device,
                presentation,
                surfaces,
                resources,
            }),
        });
        self.device_mut(device)
            .expect("additional-chain device remains alive after COM publication")
            .commit_resources(prepared)
            .expect("exclusive resource preparation remains current after chain publication");
        Ok(())
    }

    pub(in crate::process::windows::d3d8) fn swap_chain_call(
        &mut self,
        memory: &mut AddressSpace,
        windows: &WindowContext32,
        pointer: GuestAddress,
        slot: u8,
        args: &[u32],
    ) -> Result<Option<u32>, DispatchError32> {
        let ObjectKind32::SwapChain(chain) = &self.object(pointer)?.kind else {
            return Err(HResult::INVALID_CALL.into());
        };
        let device = chain.device;
        let presentation = chain.presentation;
        match slot {
            3 => {
                let surface = chain.resources[0];
                self.present_guest(
                    memory,
                    windows,
                    device,
                    surface,
                    u64::from(pointer.0),
                    presentation.window,
                    presentation.swap_effect,
                    presentation.interval,
                    args,
                )?;
            }
            4 => {
                if args[2] != 0 {
                    return Err(HResult::INVALID_CALL.into());
                }
                let surface = chain
                    .resources
                    .get(args[1] as usize)
                    .copied()
                    .ok_or(HResult::INVALID_CALL)?;
                self.acquire_surface(memory, device, pointer, surface, args[3])?;
            }
            _ => return Ok(None),
        }
        Ok(Some(0))
    }
}

fn back_buffer_description(
    presentation: PresentationParameters32,
) -> Result<ResourceDesc, DispatchError32> {
    Ok(ResourceDesc::Surface(SurfaceDesc {
        width: presentation.width,
        height: presentation.height,
        format: Format::try_from(presentation.format).map_err(DeviceError::from)?,
        usage: Usage(Usage::RENDER_TARGET),
        pool: Pool::Default,
        multisample: presentation.multisample,
        lockable: presentation.flags & 1 != 0,
    }))
}
