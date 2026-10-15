use super::super::{
    DirectInputRuntime32,
    memory::{bytes, output, put, words},
};
use crate::{
    input::{
        dinput::{DeviceKind32, DirectInputResult32 as H, InterfaceKind32, format::DeviceObject32},
        keyboard,
    },
    memory::{AddressSpace, GuestAddress},
};
impl DirectInputRuntime32 {
    pub(in crate::process::windows::dinput) fn property(
        &mut self,
        memory: &mut AddressSpace,
        pointer: GuestAddress,
        args: &[u32],
        set: bool,
        interface: InterfaceKind32,
    ) -> Result<u32, H> {
        let property = args[1];
        if !(1..=26).contains(&property) {
            return Err(H::UNSUPPORTED);
        }
        let [size, header, object, how] = words(memory, args[2])?;
        if header != 16 || how > 3 || how == 0 && object != 0 {
            return Err(H::INVALID_PARAM);
        }
        let expected = match property {
            4 | 18 | 19 => 24,
            12 => 552,
            13 | 14 | 20 | 25 | 26 => 536,
            _ => 20,
        };
        if size != expected {
            return Err(H::INVALID_PARAM);
        }
        let selected = if how == 0 {
            None
        } else {
            Some(self.resolve_object(pointer, object, how)?)
        };
        if set && property != 7 {
            self.device(pointer)?.native.check_unacquired()?;
        }
        if set {
            let [value] = words(memory, args[2].checked_add(16).ok_or(H::INVALID_PARAM)?)?;
            match property {
                1 if how == 0 => self.device_mut(pointer)?.native.set_buffer_size(value)?,
                2 if how == 0 && value <= 1 => {
                    self.device_mut(pointer)?.native.set_axis_mode(value == 1)?
                }
                7 if how == 0 => self.device_mut(pointer)?.native.set_gain(value)?,
                4 | 5 | 6 | 10 => {
                    let index = match selected {
                        None => None,
                        Some(DeviceObject32::Axis { index, .. }) => Some(index),
                        _ => return Err(H::OBJECT_NOT_FOUND),
                    };
                    if self.device(pointer)?.native.description.axes == 0 {
                        return Err(H::UNSUPPORTED);
                    }
                    let indices = if let Some(index) = index {
                        index..index + 1
                    } else {
                        0..self.device(pointer)?.native.description.axes
                    };
                    let mut updated = Vec::new();
                    updated
                        .try_reserve_exact(indices.len())
                        .map_err(|_| H::OUT_OF_MEMORY)?;
                    for index in indices {
                        let mut axis = self.device(pointer)?.native.axis(index)?;
                        match property {
                            4 => {
                                let [maximum] = words(memory, args[2] + 20)?;
                                axis.minimum = value as i32;
                                axis.maximum = maximum as i32;
                                if axis.minimum >= axis.maximum {
                                    return Err(H::INVALID_PARAM);
                                }
                            }
                            5 => {
                                if value > 10000 {
                                    return Err(H::INVALID_PARAM);
                                }
                                axis.deadzone = value;
                            }
                            6 => {
                                if value > 10000 {
                                    return Err(H::INVALID_PARAM);
                                }
                                axis.saturation = value;
                            }
                            10 => {
                                if value > 1 {
                                    return Err(H::INVALID_PARAM);
                                }
                                axis.raw = value == 1;
                            }
                            _ => unreachable!(),
                        }
                        updated.push((index, axis));
                    }
                    for (index, axis) in updated {
                        self.device_mut(pointer)?
                            .native
                            .set_axis(Some(index), axis)
                            .expect("axis properties prevalidated");
                    }
                }
                22 => {
                    let object = selected.ok_or(H::INVALID_PARAM)?;
                    let format = self.device_mut(pointer)?.native.format_mut()?;
                    let binding = format
                        .bindings_mut()
                        .iter_mut()
                        .find(|binding| binding.object == Some(object))
                        .ok_or(H::OBJECT_NOT_FOUND)?;
                    binding.app_data = value;
                }
                _ => return Err(H::UNSUPPORTED),
            }
            return Ok(0);
        }
        output(memory, args[2], size as usize)?;
        let mut result = bytes(size as usize)?;
        for (index, value) in [size, header, object, how].into_iter().enumerate() {
            put(&mut result, index * 4, value);
        }
        let device = self.device(pointer)?;
        match property {
            1 if how == 0 => put(&mut result, 16, device.native.requested_buffer()),
            2 if how == 0 => put(&mut result, 16, u32::from(device.native.relative())),
            3 => put(&mut result, 16, 1),
            4 | 5 | 6 | 10 | 18 | 19 => {
                let index = match selected {
                    Some(DeviceObject32::Axis { index, .. }) => index,
                    None if device.native.description.axes != 0 => 0,
                    _ => return Err(H::OBJECT_NOT_FOUND),
                };
                let axis = device.native.axis(index)?;
                match property {
                    4 => {
                        put(&mut result, 16, axis.minimum as u32);
                        put(&mut result, 20, axis.maximum as u32);
                    }
                    5 => put(&mut result, 16, axis.deadzone),
                    6 => put(&mut result, 16, axis.saturation),
                    10 => put(&mut result, 16, u32::from(axis.raw)),
                    18 | 19 => {
                        put(&mut result, 16, (-32768i32) as u32);
                        put(&mut result, 20, 32767);
                    }
                    _ => unreachable!(),
                }
            }
            7 if how == 0 => put(&mut result, 16, device.native.gain()),
            13 | 14 => {
                let name = self.device_name(device.kind);
                for (index, unit) in name.into_iter().take(259).enumerate() {
                    result[16 + index * 2..18 + index * 2].copy_from_slice(&unit.to_le_bytes());
                }
            }
            15 => {
                if let DeviceKind32::Controller(slot) = device.kind {
                    put(&mut result, 16, u32::from(slot));
                } else {
                    return Err(H::UNSUPPORTED);
                }
            }
            20 => {
                let Some(DeviceObject32::Key(scan)) = selected else {
                    return Err(H::OBJECT_NOT_FOUND);
                };
                for (index, unit) in keyboard::key_name(scan)
                    .encode_utf16()
                    .take(259)
                    .enumerate()
                {
                    result[16 + index * 2..18 + index * 2].copy_from_slice(&unit.to_le_bytes());
                }
            }
            22 => {
                let object = selected.ok_or(H::INVALID_PARAM)?;
                let binding = device
                    .native
                    .format()
                    .and_then(|format| {
                        format
                            .bindings()
                            .iter()
                            .find(|binding| binding.object == Some(object))
                    })
                    .ok_or(H::OBJECT_NOT_FOUND)?;
                put(&mut result, 16, binding.app_data);
            }
            23 => {
                let Some(DeviceObject32::Key(scan)) = selected else {
                    return Err(H::OBJECT_NOT_FOUND);
                };
                let value = if self.object(pointer)?.version >= 0x801 && scan & 0x80 != 0 {
                    0xe0 | (u32::from(scan & 0x7f) << 8)
                } else {
                    u32::from(scan)
                };
                put(&mut result, 16, value);
            }
            24 => return Err(H::UNSUPPORTED),
            25 => {
                memory.write(GuestAddress(args[2]), &result)?;
                return Ok(1);
            }
            _ => return Err(H::UNSUPPORTED),
        }
        let _ = interface;
        memory.write(GuestAddress(args[2]), &result)?;
        Ok(0)
    }
    pub(in crate::process::windows::dinput) fn resolve_object(
        &self,
        pointer: GuestAddress,
        value: u32,
        how: u32,
    ) -> Result<DeviceObject32, H> {
        let device = self.device(pointer)?;
        let result = match how {
            1 => device
                .native
                .format()
                .and_then(|format| format.object_by_offset(value)),
            2 => device.native.description.objects().find(|object| {
                object.instance() == ((value >> 8) & 0xffff) as u16
                    && object.object_type(device.native.relative()) & value & 255 != 0
            }),
            3 => crate::input::dinput::format::usage_object(device.native.description, value),
            _ => None,
        };
        result
            .filter(|object| {
                device
                    .native
                    .description
                    .objects()
                    .any(|candidate| candidate == *object)
            })
            .ok_or(H::OBJECT_NOT_FOUND)
    }
}
