use super::super::wide;
use crate::process::wasm::mutate;

pub(crate) fn resume_dinput_file_call(low: u32, high: u32, token: u32) -> u32 {
    mutate(|instance| instance.resume_dinput_file_call(wide(low, high), token))
}

pub(crate) fn cancel_dinput_file_call(low: u32, high: u32, token: u32) -> u32 {
    mutate(|instance| instance.cancel_dinput_file_call(wide(low, high), token))
}
