use super::{
    D3D8Runtime32,
    fpu::FpuSetup32,
    memory::{check_output, read_words, word, write_words},
    objects::{Device32, Object32, ObjectKind32},
};
use crate::process::windows::objects::{GuestComCreation32, InterfaceLayout32};
use crate::{
    cpu::dbt::GateSpec,
    graphics::d3d8::{
        api::{HResult, InterfaceKind, PresentationParameters32, SDK_VERSION, validate_behavior},
        device::{Device, DeviceDescription},
        resources::Format,
    },
    memory::{AddressSpace, GuestAddress},
    windows::{
        com::ReferenceState32,
        window::{WindowContext32, WindowHandle32},
    },
};

impl D3D8Runtime32 {
    pub(super) fn create_direct3d(
        &mut self,
        memory: &mut AddressSpace,
        version: u32,
        gates: &[GateSpec],
    ) -> Result<u32, HResult> {
        if !matches!(version, 120 | SDK_VERSION) {
            return Ok(0);
        }
        self.initialize_methods(gates)?;
        let slot = self.vacant_record()?;
        let interface = InterfaceKind::Direct3D;
        let identity = self.com.create(
            memory,
            slot as u64,
            &[InterfaceLayout32 {
                iid: interface.iid(),
                methods: &self.methods[interface as usize - 1],
            }],
        )?;
        self.records[slot] = Some(Object32 {
            pointer: identity.identity_pointer,
            interface,
            kind: ObjectKind32::Direct3D,
            private_unknowns: Vec::new(),
        });
        Ok(identity.identity_pointer.0)
    }

    pub(super) fn direct3d_call(
        &mut self,
        memory: &mut AddressSpace,
        windows: Option<&WindowContext32>,
        slot: u8,
        args: &[u32],
        epoch: u64,
        fpu: &mut FpuSetup32,
    ) -> Result<u32, HResult> {
        let pointer = GuestAddress(args[0]);
        if self.object(pointer)?.interface != InterfaceKind::Direct3D {
            return Err(HResult::INVALID_CALL);
        }
        let profile = &self.config.adapter;
        match slot {
            3 => Err(HResult::NOT_AVAILABLE), // native software driver callbacks are not executable host drivers.
            4 => Ok(1),
            5 => {
                adapter(args[1])?;
                if args[2] & !2 != 0 {
                    return Err(HResult::INVALID_CALL);
                }
                check_output(
                    memory,
                    args[3],
                    crate::graphics::d3d8::api::ADAPTER_IDENTIFIER_BYTES,
                )?;
                memory.write(GuestAddress(args[3]), &profile.identifier())?;
                Ok(0)
            }
            6 => {
                adapter(args[1])?;
                Ok(profile.modes.len() as u32)
            }
            7 => {
                adapter(args[1])?;
                let mode = profile
                    .modes
                    .get(args[2] as usize)
                    .ok_or(HResult::INVALID_CALL)?;
                write_words(memory, args[3], &mode.words())?;
                Ok(0)
            }
            8 => {
                adapter(args[1])?;
                write_words(memory, args[2], &profile.display.words())?;
                Ok(0)
            }
            9 => {
                adapter(args[1])?;
                device_type(args[2])?;
                profile.check_device_type(format(args[3])?, format(args[4])?, args[5] != 0)?;
                Ok(0)
            }
            10 => {
                adapter(args[1])?;
                device_type(args[2])?;
                format(args[3])?;
                profile.check_format(
                    crate::graphics::d3d8::resources::Usage(args[4]),
                    args[5],
                    format(args[6])?,
                )?;
                Ok(0)
            }
            11 => {
                adapter(args[1])?;
                device_type(args[2])?;
                profile.check_multisample(format(args[3])?, args[5])?;
                Ok(0)
            }
            12 => {
                adapter(args[1])?;
                device_type(args[2])?;
                format(args[3])?;
                profile.check_depth_stencil(format(args[4])?, format(args[5])?)?;
                Ok(0)
            }
            13 => {
                adapter(args[1])?;
                device_type(args[2])?;
                write_words(memory, args[3], &profile.caps())?;
                Ok(0)
            }
            14 => {
                adapter(args[1])?;
                Ok(1)
            } // virtual primary monitor identity shared by the desktop host.
            15 => self.create_device(
                memory,
                windows.ok_or(HResult::INVALID_CALL)?,
                args,
                epoch,
                fpu,
            ),
            _ => Err(HResult::INVALID_CALL),
        }
    }

    fn create_device(
        &mut self,
        memory: &mut AddressSpace,
        windows: &WindowContext32,
        args: &[u32],
        epoch: u64,
        fpu: &mut FpuSetup32,
    ) -> Result<u32, HResult> {
        check_output(memory, args[6], 4)?;
        adapter(args[1])?;
        device_type(args[2])?;
        validate_behavior(args[4])?;
        if self.device_slots.len() + self.retired_devices.len() >= self.config.max_devices as usize
        {
            return Err(HResult::OUT_OF_MEMORY);
        }
        let focus = WindowHandle32::try_from(args[3]).map_err(|_| HResult::INVALID_CALL)?;
        windows.get(focus).map_err(|_| HResult::INVALID_CALL)?;
        let input = PresentationParameters32::decode(read_words(memory, args[5])?);
        let window = WindowHandle32::try_from(if input.window == 0 {
            args[3]
        } else {
            input.window
        })
        .map_err(|_| HResult::INVALID_CALL)?;
        let extent = windows
            .get(window)
            .map_err(|_| HResult::INVALID_CALL)?
            .description
            .extent;
        let presentation = input.normalize(args[3], extent, &self.config.adapter, false)?;
        if args[4] & 2 == 0 {
            *fpu = FpuSetup32::Entered;
        }
        let backend = Device::new(
            epoch,
            DeviceDescription {
                width: presentation.width,
                height: presentation.height,
                format: format(presentation.format)?,
                back_buffers: presentation.back_buffers,
                multisample: presentation.multisample,
                depth: if presentation.auto_depth {
                    Some(format(presentation.depth_format)?)
                } else {
                    None
                },
                window: presentation.window,
                interval: presentation.interval,
                swap_effect: presentation.swap_effect,
                lockable_back_buffer: presentation.flags & 1 != 0,
                behavior: args[4],
            },
            self.config.device,
        )
        .map_err(HResult::from)?;
        let parent = self.object(GuestAddress(args[0]))?.pointer;
        let slot = self.vacant_record()?;
        let interface = InterfaceKind::Device;
        let mut implicit = Vec::new();
        implicit
            .try_reserve_exact(4)
            .map_err(|_| HResult::OUT_OF_MEMORY)?;
        let mut resource_holds = Vec::new();
        resource_holds
            .try_reserve_exact((self.config.device.state_blocks as usize + 2) * 25 + 4)
            .map_err(|_| HResult::OUT_OF_MEMORY)?;
        let words = presentation.words();
        let normalized: Vec<_> = words
            .iter()
            .enumerate()
            .map(|(index, value)| word(args[5] + index as u32 * 4, *value))
            .collect();
        let identity = self.com.create_with_lifetime(
            memory,
            slot as u64,
            &[InterfaceLayout32 {
                iid: interface.iid(),
                methods: &self.methods[interface as usize - 1],
            }],
            GuestComCreation32 {
                output: Some((GuestAddress(args[6]), interface.iid())),
                writes: &normalized,
                initial: ReferenceState32 {
                    references: 1,
                    holds: 0,
                },
                retain: &[parent],
            },
        )?;
        self.records[slot] = Some(Object32 {
            pointer: identity.identity_pointer,
            interface,
            private_unknowns: Vec::new(),
            kind: ObjectKind32::Device(Box::new(Device32 {
                backend,
                parent,
                focus_window: args[3],
                behavior: args[4],
                presentation,
                implicit,
                resource_holds,
                target_holds: [None; 2],
            })),
        });
        self.device_slots.push(slot);
        Ok(0)
    }
}

pub(super) fn adapter(value: u32) -> Result<(), HResult> {
    if value == 0 {
        Ok(())
    } else {
        Err(HResult::INVALID_CALL)
    }
}
pub(super) fn device_type(value: u32) -> Result<(), HResult> {
    if value == 1 {
        Ok(())
    } else if (2..=3).contains(&value) {
        Err(HResult::NOT_AVAILABLE)
    } else {
        Err(HResult::INVALID_CALL)
    }
}
pub(super) fn format(value: u32) -> Result<Format, HResult> {
    Format::try_from(value).map_err(|_| HResult::NOT_AVAILABLE)
}
