use super::super::{
    DirectInputRuntime32,
    memory::{bytes, output, put, words},
};
use crate::{
    input::dinput::{
        ControllerKind32, DeviceKind32, DirectInputResult32 as H, InterfaceKind32,
        api::{SYSTEM_KEYBOARD, SYSTEM_MOUSE},
        format::DeviceObject32,
    },
    memory::{AddressSpace, GuestAddress},
};
impl DirectInputRuntime32 {
    pub(in crate::process::windows::dinput) fn device_name(&self, kind: DeviceKind32) -> Vec<u16> {
        match kind {
            DeviceKind32::Keyboard => "Ring3 keyboard".encode_utf16().collect(),
            DeviceKind32::Mouse => "Ring3 mouse".encode_utf16().collect(),
            DeviceKind32::Controller(slot) => self.controllers[usize::from(slot)]
                .as_ref()
                .map_or_else(Vec::new, |controller| controller.name.clone()),
        }
    }
    pub(in crate::process::windows::dinput) fn device_guid(
        &self,
        kind: DeviceKind32,
        product: bool,
    ) -> [u8; 16] {
        match kind {
            DeviceKind32::Keyboard => SYSTEM_KEYBOARD,
            DeviceKind32::Mouse => SYSTEM_MOUSE,
            DeviceKind32::Controller(slot) => {
                let controller = self.controllers[usize::from(slot)].as_ref().unwrap();
                if product {
                    controller.product
                } else {
                    controller.instance
                }
            }
        }
    }
    pub(in crate::process::windows::dinput) fn device_type(
        &self,
        kind: DeviceKind32,
        version8: bool,
    ) -> u32 {
        match kind {
            DeviceKind32::Keyboard => 0x400 | if version8 { 0x13 } else { 3 },
            DeviceKind32::Mouse => 0x200 | if version8 { 0x12 } else { 2 },
            DeviceKind32::Controller(slot) => {
                if version8 {
                    0x100
                        | match self.controllers[usize::from(slot)].as_ref().unwrap().kind {
                            ControllerKind32::Joystick => 0x14,
                            ControllerKind32::Gamepad => 0x15,
                            ControllerKind32::Driving => 0x16,
                            ControllerKind32::Flight => 0x17,
                            ControllerKind32::FirstPerson => 0x18,
                        }
                } else {
                    0x104
                }
            }
        }
    }
    pub(in crate::process::windows::dinput) fn capabilities(
        &self,
        memory: &mut AddressSpace,
        pointer: GuestAddress,
        address: u32,
    ) -> Result<u32, H> {
        let [size] = words(memory, address)?;
        if !matches!(size, 24 | 44) {
            return Err(H::INVALID_PARAM);
        }
        output(memory, address, size as usize)?;
        let device = self.device(pointer)?;
        let mut result = bytes(size as usize)?;
        for (index, value) in [
            size,
            5 | if device.haptics.is_some() { 0xff00 } else { 0 },
            self.device_type(
                device.kind,
                self.object(pointer)?.interface.generation() == 8,
            ),
            u32::from(device.native.description.axes),
            u32::from(device.native.description.buttons),
            u32::from(device.native.description.povs),
        ]
        .into_iter()
        .enumerate()
        {
            put(&mut result, index * 4, value);
        }
        if size == 44 && device.haptics.is_some() {
            put(&mut result, 24, 1000);
            put(&mut result, 28, 1);
            put(&mut result, 40, 0x10000);
        }
        memory.write(GuestAddress(address), &result)?;
        Ok(0)
    }
    pub(in crate::process::windows::dinput) fn instance_bytes(
        &self,
        kind: DeviceKind32,
        wide: bool,
        version8: bool,
        size: usize,
    ) -> Result<Vec<u8>, H> {
        let minimum = if wide { 1080 } else { 560 };
        if ![minimum, minimum + 20].contains(&size) {
            return Err(H::INVALID_PARAM);
        }
        let mut result = bytes(size)?;
        put(&mut result, 0, size as u32);
        result[4..20].copy_from_slice(&self.device_guid(kind, false));
        result[20..36].copy_from_slice(&self.device_guid(kind, true));
        put(&mut result, 36, self.device_type(kind, version8));
        let name = self.device_name(kind);
        let ansi = if wide {
            None
        } else {
            Some(super::names::ansi_name(&name, self.ansi, 259)?)
        };
        for at in [40, 40 + 260 * if wide { 2 } else { 1 }] {
            if let Some(ansi) = &ansi {
                result[at..at + ansi.len()].copy_from_slice(ansi);
            } else {
                for (index, unit) in name.iter().take(259).enumerate() {
                    result[at + index * 2..at + index * 2 + 2].copy_from_slice(&unit.to_le_bytes());
                }
            }
        }
        if size > minimum {
            if matches!(kind,DeviceKind32::Controller(slot) if self.haptic_capabilities[slot as usize].is_some())
            {
                result[minimum..minimum + 16].copy_from_slice(&[
                    0x52, 0x33, 0x46, 0x46, 0x08, 0, 0x40, 0x40, 0x80, 0, 0, 0, 0, 0, 0, 1,
                ]);
            }
            let usage = match kind {
                DeviceKind32::Keyboard => 6,
                DeviceKind32::Mouse => 2,
                _ => 4,
            };
            result[minimum + 16..minimum + 18].copy_from_slice(&1u16.to_le_bytes());
            result[minimum + 18..minimum + 20].copy_from_slice(&(usage as u16).to_le_bytes());
        }
        Ok(result)
    }
    pub(in crate::process::windows::dinput) fn device_info(
        &self,
        memory: &mut AddressSpace,
        pointer: GuestAddress,
        address: u32,
        interface: InterfaceKind32,
    ) -> Result<u32, H> {
        let [size] = words(memory, address)?;
        let bytes = self.instance_bytes(
            self.device(pointer)?.kind,
            interface.wide(),
            interface.generation() == 8,
            size as usize,
        )?;
        output(memory, address, bytes.len())?;
        memory.write(GuestAddress(address), &bytes)?;
        Ok(0)
    }
    pub(in crate::process::windows::dinput) fn object_bytes(
        &self,
        pointer: GuestAddress,
        object: DeviceObject32,
        interface: InterfaceKind32,
        size: usize,
    ) -> Result<Vec<u8>, H> {
        let minimum = if interface.wide() { 552 } else { 292 };
        if size != minimum && size != minimum + 24 {
            return Err(H::INVALID_PARAM);
        }
        let mut result = bytes(size)?;
        put(&mut result, 0, size as u32);
        result[4..20].copy_from_slice(&object_guid(object));
        let device = self.device(pointer)?;
        let offset = device
            .native
            .format()
            .and_then(|format| format.offset_for(object))
            .unwrap_or_else(|| match object {
                DeviceObject32::Key(index) => u32::from(index),
                DeviceObject32::Button(index) => 12 + u32::from(index),
                DeviceObject32::Axis { index, .. } => u32::from(index) * 4,
                DeviceObject32::Pov(index) => 32 + u32::from(index) * 4,
            });
        put(&mut result, 20, offset);
        put(
            &mut result,
            24,
            object.object_type(device.native.relative()),
        );
        let actuator = matches!(object,DeviceObject32::Axis{index,aspect:0} if device.haptics.as_ref().is_some_and(|haptics|index<haptics.capabilities().axes));
        let trigger = matches!(object, DeviceObject32::Button(_)) && device.haptics.is_some();
        let aspect = match object {
            DeviceObject32::Axis { aspect, .. } => u32::from(aspect + 1) * 0x100,
            _ => 0,
        };
        put(
            &mut result,
            28,
            aspect | u32::from(actuator) | (u32::from(trigger) * 2),
        );
        let (name, _usage_page, _usage) = match object {
            DeviceObject32::Key(scan) => (crate::input::keyboard::key_name(scan), 7, 0),
            DeviceObject32::Button(index) => (format!("Button {}", index + 1), 9, index + 1),
            DeviceObject32::Axis { index, .. } => (
                format!(
                    "{} axis",
                    ["X", "Y", "Z", "Rx", "Ry", "Rz", "Slider 1", "Slider 2"][usize::from(index)]
                ),
                1,
                0x30 + u16::from(index),
            ),
            DeviceObject32::Pov(index) => (format!("POV {}", index + 1), 1, 0x39),
        };
        if interface.wide() {
            for (index, unit) in name.encode_utf16().take(259).enumerate() {
                result[32 + index * 2..34 + index * 2].copy_from_slice(&unit.to_le_bytes());
            }
        } else {
            let units = name.encode_utf16().collect::<Vec<_>>();
            let ansi = super::names::ansi_name(&units, self.ansi, 259)?;
            result[32..32 + ansi.len()].copy_from_slice(&ansi);
        }
        if size > minimum {
            if actuator {
                put(&mut result, minimum, 10000);
                put(&mut result, minimum + 4, 1);
            }
            let (usage_page, usage) = crate::input::dinput::format::object_usage(object);
            result[minimum + 12..minimum + 14].copy_from_slice(&usage_page.to_le_bytes());
            result[minimum + 14..minimum + 16].copy_from_slice(&usage.to_le_bytes());
        }
        Ok(result)
    }
    pub(in crate::process::windows::dinput) fn object_info(
        &self,
        memory: &mut AddressSpace,
        pointer: GuestAddress,
        address: u32,
        object: u32,
        how: u32,
        interface: InterfaceKind32,
    ) -> Result<u32, H> {
        let [size] = words(memory, address)?;
        let object = self.resolve_object(pointer, object, how)?;
        let bytes = self.object_bytes(pointer, object, interface, size as usize)?;
        output(memory, address, bytes.len())?;
        memory.write(GuestAddress(address), &bytes)?;
        Ok(0)
    }
}
fn object_guid(object: DeviceObject32) -> [u8; 16] {
    let mut guid = [
        0xe0, 2, 0x6d, 0xa3, 0xf3, 0xc9, 0xcf, 0x11, 0xbf, 0xc7, 0x44, 0x45, 0x53, 0x54, 0, 0,
    ];
    match object {
        DeviceObject32::Key(_) => {
            guid = [
                0x20, 0x82, 0x72, 0x55, 0x3c, 0xd3, 0xcf, 0x11, 0xbf, 0xc7, 0x44, 0x45, 0x53, 0x54,
                0, 0,
            ]
        }
        DeviceObject32::Button(_) => guid[0] = 0xf0,
        DeviceObject32::Pov(_) => guid[0] = 0xf2,
        DeviceObject32::Axis { index, .. } => {
            guid[0] = [0xe0, 0xe1, 0xe2, 0xf4, 0xf5, 0xe3, 0xe4, 0xe4][usize::from(index)]
        }
    }
    guid
}
