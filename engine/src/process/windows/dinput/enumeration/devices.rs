use super::super::{
    DirectInputRuntime32,
    memory::{output, read},
    objects::{Device32, ObjectKind32},
};
use crate::{
    input::{
        dinput::{
            DeviceKind32, DirectInputResult32 as H, InterfaceKind32,
            api::{SYSTEM_KEYBOARD, SYSTEM_MOUSE},
            device::DirectInputDevice32,
            format::DeviceDescription32,
        },
        source::InputSource32,
    },
    memory::{AddressSpace, GuestAddress},
    process::WindowsProviderOutcome32,
};
impl DirectInputRuntime32 {
    pub(in crate::process::windows::dinput) fn identify(
        &self,
        guid: [u8; 16],
    ) -> Result<DeviceKind32, H> {
        if guid == SYSTEM_KEYBOARD {
            return Ok(DeviceKind32::Keyboard);
        }
        if guid == SYSTEM_MOUSE {
            return Ok(DeviceKind32::Mouse);
        }
        self.controllers
            .iter()
            .position(|descriptor| {
                descriptor
                    .as_ref()
                    .is_some_and(|descriptor| descriptor.instance == guid)
            })
            .map(|slot| DeviceKind32::Controller(slot as u8))
            .ok_or(H::DEVICE_NOT_REGISTERED)
    }
    pub(in crate::process::windows::dinput) fn create_device(
        &mut self,
        memory: &mut AddressSpace,
        parent: GuestAddress,
        interface: InterfaceKind32,
        guid: u32,
        out: u32,
        outer: u32,
    ) -> Result<u32, H> {
        output(memory, out, 4)?;
        if outer != 0 {
            return Err(H::NO_AGGREGATION);
        }
        if self
            .records
            .iter()
            .flatten()
            .filter(|object| matches!(object.kind, ObjectKind32::Device(_)))
            .count()
            >= self.config.max_devices as usize
        {
            return Err(H::OUT_OF_MEMORY);
        }
        let kind = self.identify(read(memory, guid)?)?;
        let description = match kind {
            DeviceKind32::Keyboard => DeviceDescription32::keyboard(),
            DeviceKind32::Mouse => DeviceDescription32::mouse(),
            DeviceKind32::Controller(slot) => {
                let controller = self.controllers[usize::from(slot)].as_ref().unwrap();
                DeviceDescription32 {
                    kind,
                    axes: controller.axes,
                    povs: controller.povs,
                    buttons: controller.buttons,
                }
            }
        };
        let version = self.object(parent)?.version;
        let native = DirectInputDevice32::new(description, self.config.max_buffer_events as usize)?;
        let haptics = match kind {
            DeviceKind32::Controller(slot) => self.haptic_capabilities[slot as usize]
                .map(crate::input::dinput::haptics::EffectDevice32::new)
                .transpose()?,
            _ => None,
        };
        let canonical = self.object(parent)?.pointer;
        self.publish(
            memory,
            interface,
            version,
            ObjectKind32::device(Device32 {
                native,
                parent: canonical,
                notification: None,
                kind,
                transfer: Vec::new(),
                haptics,
            })?,
            out,
            Some(canonical),
        )?;
        Ok(0)
    }
    #[allow(clippy::too_many_arguments)]
    pub(in crate::process::windows::dinput) fn enum_devices(
        &mut self,
        memory: &mut AddressSpace,
        source: &InputSource32,
        interface: InterfaceKind32,
        class: u32,
        procedure: u32,
        context: u32,
        flags: u32,
    ) -> Result<WindowsProviderOutcome32, H> {
        if flags & !0x0007_0101 != 0 || class > 0x1c {
            return Err(H::INVALID_PARAM);
        }
        let mut rows = Vec::new();
        rows.try_reserve_exact(10).map_err(|_| H::OUT_OF_MEMORY)?;
        {
            for kind in std::iter::once(DeviceKind32::Keyboard)
                .chain(std::iter::once(DeviceKind32::Mouse))
                .chain(
                    self.controllers
                        .iter()
                        .enumerate()
                        .filter_map(|(slot, descriptor)| {
                            descriptor
                                .as_ref()
                                .map(|_| DeviceKind32::Controller(slot as u8))
                        }),
                )
            {
                if flags & 1 != 0
                    && matches!(kind,DeviceKind32::Controller(slot) if !source.snapshot().controllers[usize::from(slot)].connected)
                {
                    continue;
                }
                if flags & 0x100 != 0
                    && !matches!(kind,DeviceKind32::Controller(slot) if self.haptic_capabilities[slot as usize].is_some())
                {
                    continue;
                }
                let device_type = self.device_type(kind, interface.generation() == 8) & 255;
                let matches = class == 0
                    || class == 1
                    || class == 2 && kind == DeviceKind32::Mouse
                    || class == 3 && kind == DeviceKind32::Keyboard
                    || class == 4 && matches!(kind, DeviceKind32::Controller(_))
                    || class == device_type;
                if matches {
                    rows.push(self.instance_bytes(
                        kind,
                        interface.wide(),
                        interface.generation() == 8,
                        if interface.wide() { 1100 } else { 580 },
                    )?);
                }
            }
        }
        self.start_enumeration(memory, procedure, context, interface, rows)
    }
}
