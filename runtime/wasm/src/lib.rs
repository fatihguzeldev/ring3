mod process;

#[cfg(test)]
mod tests;

use process::{MAX_INPUT, Session};
use std::cell::RefCell;

#[derive(Default)]
struct Bridge {
    input: Vec<u8>,
    output: Vec<u8>,
    session: Session,
}

thread_local! { static BRIDGE: RefCell<Bridge> = RefCell::new(Bridge::default()); }

// exported addresses refer only to owned buffers; callers never supply pointers.
#[must_use]
#[allow(unsafe_code)]
#[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
pub extern "C" fn ring3_input(length: u32) -> usize {
    BRIDGE.with_borrow_mut(|bridge| {
        if length as usize > MAX_INPUT {
            return 0;
        }
        bridge.input.resize(length as usize, 0);
        bridge.input.as_mut_ptr() as usize
    })
}

#[must_use]
#[allow(unsafe_code)]
#[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
pub extern "C" fn ring3_command(operation: u32, argument: u32) -> u32 {
    BRIDGE.with_borrow_mut(|bridge| {
        let input = std::mem::take(&mut bridge.input);
        let result = bridge.session.command(operation, argument, input);
        let success = u32::from(result.is_ok());
        let value = result.unwrap_or_else(|error| serde_json::json!({"error":error}));
        bridge.output = value.to_string().into_bytes();
        success
    })
}

#[must_use]
#[allow(unsafe_code)]
#[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
pub extern "C" fn ring3_output_pointer() -> usize {
    BRIDGE.with_borrow(|bridge| bridge.output.as_ptr() as usize)
}

#[must_use]
#[allow(unsafe_code)]
#[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
pub extern "C" fn ring3_output_length() -> usize {
    BRIDGE.with_borrow(|bridge| bridge.output.len())
}

#[must_use]
#[allow(unsafe_code)]
#[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
pub extern "C" fn ring3_frame_pointer() -> usize {
    BRIDGE.with_borrow(|bridge| {
        bridge
            .session
            .frame
            .as_ref()
            .map_or(0, |frame| frame.rgba.as_ptr() as usize)
    })
}

#[must_use]
#[allow(unsafe_code)]
#[cfg_attr(target_arch = "wasm32", unsafe(no_mangle))]
pub extern "C" fn ring3_frame_length() -> usize {
    BRIDGE.with_borrow(|bridge| {
        bridge
            .session
            .frame
            .as_ref()
            .map_or(0, |frame| frame.rgba.len())
    })
}
