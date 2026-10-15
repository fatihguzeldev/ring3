use super::super::super::DirectInputRuntime32;
use super::decoding::read_action_map;
use crate::{
    input::dinput::{
        DeviceKind32, DirectInputResult32 as H, InterfaceKind32, actions::build_actions,
        format::DataFormat32,
    },
    memory::{AddressSpace, GuestAddress},
};
impl DirectInputRuntime32 {
    pub(in crate::process::windows::dinput) fn action_map(
        &mut self,
        memory: &mut AddressSpace,
        pointer: GuestAddress,
        args: &[u32],
        interface: InterfaceKind32,
        set: bool,
    ) -> Result<u32, H> {
        self.device(pointer)?.native.check_unacquired()?;
        let mut map = read_action_map(memory, args[1], interface.wide())?;
        if if set {
            args[3] & !3 != 0
        } else {
            !matches!(args[3], 0 | 1 | 2 | 4)
        } {
            return Err(H::INVALID_PARAM);
        }
        let kind = self.device(pointer)?.kind;
        let instance = self.device_guid(kind, false);
        let description = self.device(pointer)?.native.description;
        if !set {
            let mapped = build_actions(description, instance, &mut map.actions, args[3] & 1 != 0)?;
            let encoded = map.encoded_actions()?;
            memory.write_spans(&[
                (GuestAddress(map.action_address), &encoded),
                (GuestAddress(map.address), &map.header),
            ])?;
            return Ok(if mapped == 0 { 1 } else { 0 });
        }
        let mut bindings = Vec::new();
        bindings
            .try_reserve_exact(map.actions.len())
            .map_err(|_| H::OUT_OF_MEMORY)?;
        for (index, action) in map.actions.iter().enumerate() {
            if action.instance != instance || action.flags & 5 != 0 {
                continue;
            }
            let object = description
                .objects()
                .find(|object| {
                    object.instance() == ((action.object_id >> 8) & 0xffff) as u16
                        && object.object_type(kind == DeviceKind32::Mouse) & action.object_id & 255
                            != 0
                })
                .ok_or(H::OBJECT_NOT_FOUND)?;
            bindings.push((index as u32 * 4, object, action.app_data));
        }
        let format = DataFormat32::from_actions(
            description,
            map.data_size,
            kind == DeviceKind32::Mouse,
            &bindings,
        )?;
        let mut axis_updates = Vec::new();
        axis_updates
            .try_reserve_exact(usize::from(description.axes))
            .map_err(|_| H::OUT_OF_MEMORY)?;
        if kind != DeviceKind32::Mouse {
            for &(offset, object, _) in &bindings {
                if let crate::input::dinput::format::DeviceObject32::Axis { index, .. } = object
                    && map.actions[(offset / 4) as usize].flags & 8 == 0
                {
                    if map.minimum >= map.maximum {
                        return Err(H::INVALID_PARAM);
                    }
                    let mut properties = self.device(pointer)?.native.axis(index)?;
                    properties.minimum = map.minimum;
                    properties.maximum = map.maximum;
                    axis_updates.push((index, properties));
                }
            }
        }
        let native = &mut self.device_mut(pointer)?.native;
        native.set_format(format)?;
        native.set_buffer_size(map.buffer)?;
        for (index, properties) in axis_updates {
            native
                .set_axis(Some(index), properties)
                .expect("action axis range prevalidated");
        }
        let index = super::super::super::enumeration::device_index(kind);
        self.used_devices[index] = true;
        self.owned_devices[index] = args[3] & 1 == 0;
        Ok(0)
    }
}
