use super::{header, word};
use crate::{
    input::dinput::{ControllerDescriptor32, ControllerKind32},
    memory::{GuestAddress, PageRange},
    process::{
        DirectInputConfig32, HostError,
        wasm::{mutate, windows::input},
    },
};

pub(crate) fn configure_dinput(length: u32) -> u32 {
    mutate(|instance| {
        let bytes = input(instance, length)?;
        header(bytes, b"R3CI", 64)?;
        if bytes.len() != 64 || bytes[44..].iter().any(|byte| *byte != 0) {
            return Err(HostError::InvalidRequest);
        }
        let config = DirectInputConfig32 {
            objects: PageRange::new(GuestAddress(word(bytes, 16)), word(bytes, 20))
                .map_err(HostError::Memory)?,
            scratch: PageRange::new(GuestAddress(word(bytes, 24)), word(bytes, 28))
                .map_err(HostError::Memory)?,
            max_objects: word(bytes, 32),
            max_devices: word(bytes, 36),
            max_buffer_events: word(bytes, 40),
        };
        instance.configure_dinput(config)
    })
}

pub(crate) fn register_dinput_module(base: u32) -> u32 {
    mutate(|instance| instance.register_dinput_module(base))
}

pub(crate) fn register_dinput8_module(base: u32) -> u32 {
    mutate(|instance| instance.register_dinput8_module(base))
}

pub(crate) fn register_input_controller(length: u32) -> u32 {
    mutate(|instance| {
        let bytes = input(instance, length)?;
        header(bytes, b"R3ID", 80)?;
        let slot = u8::try_from(word(bytes, 16)).map_err(|_| HostError::InvalidRequest)?;
        let axes = u8::try_from(word(bytes, 20)).map_err(|_| HostError::InvalidRequest)?;
        let povs = u8::try_from(word(bytes, 24)).map_err(|_| HostError::InvalidRequest)?;
        let buttons = u16::try_from(word(bytes, 28)).map_err(|_| HostError::InvalidRequest)?;
        let kind =
            ControllerKind32::try_from(word(bytes, 32)).map_err(|_| HostError::InvalidRequest)?;
        let units = word(bytes, 36) as usize;
        if units == 0
            || units > 259
            || bytes.len() != 80 + units * 2
            || bytes[72..80].iter().any(|byte| *byte != 0)
        {
            return Err(HostError::InvalidRequest);
        }
        let mut name = Vec::new();
        name.try_reserve_exact(units)
            .map_err(|_| HostError::Infrastructure)?;
        for pair in bytes[80..].chunks_exact(2) {
            let value = u16::from_le_bytes(pair.try_into().unwrap());
            if value == 0 {
                return Err(HostError::InvalidRequest);
            }
            name.push(value);
        }
        let descriptor = ControllerDescriptor32 {
            instance: bytes[40..56].try_into().unwrap(),
            product: bytes[56..72].try_into().unwrap(),
            name,
            axes,
            povs,
            buttons,
            kind,
        };
        instance.register_input_controller(slot, descriptor)
    })
}
