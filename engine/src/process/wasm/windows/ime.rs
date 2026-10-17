use crate::{
    abi::windows::ime::decode_composition_event,
    process::{HostError, wasm::mutate},
    windows::window::WindowHandle32,
};

pub(crate) fn submit_ime_composition(window: u32, length: u32) -> u32 {
    mutate(|instance| {
        let event = decode_composition_event(super::input(instance, length)?)
            .map_err(|_| HostError::InvalidRequest)?;
        let window = WindowHandle32::try_from(window).map_err(|_| HostError::InvalidRequest)?;
        instance.submit_ime_composition(window, event)
    })
}
