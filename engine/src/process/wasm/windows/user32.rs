use super::{record, wide};
use crate::{
    abi::media::MediaRecordKind,
    memory::{GuestAddress, PageRange},
    process::{HostError, wasm::mutate},
    windows::user32::{Message32, Point32, User32Limits32},
};

pub(crate) fn configure_user32(
    classes: u32,
    windows: u32,
    messages: u32,
    continuations: u32,
    scratch_base: u32,
    scratch_pages: u32,
) -> u32 {
    mutate(|instance| {
        instance.configure_user32(
            User32Limits32 {
                classes: classes as usize,
                windows: windows as usize,
                messages: messages as usize,
                continuations: continuations as usize,
            },
            PageRange::new(GuestAddress(scratch_base), scratch_pages).map_err(HostError::Memory)?,
        )
    })
}
pub(crate) fn register_user32_module(base: u32) -> u32 {
    mutate(|instance| instance.register_user32_module(base))
}
pub(crate) fn set_user32_keyboard_state(flags: u32) -> u32 {
    mutate(|instance| {
        if flags & !7 != 0 {
            return Err(HostError::InvalidRequest);
        }
        instance.set_user32_keyboard_state(flags & 1 != 0, flags & 2 != 0, flags & 4 != 0)
    })
}
pub(crate) fn post_user32_message(
    window: u32,
    message: u32,
    wparam: u32,
    lparam: u32,
    time_ms: u32,
    x: u32,
    y: u32,
) -> u32 {
    mutate(|instance| {
        instance.post_user32_message(Message32 {
            window,
            message,
            wparam,
            lparam,
            time_ms,
            point: Point32 {
                x: x as i32,
                y: y as i32,
            },
            thread_id: 1,
        })
    })
}
pub(crate) fn waiting_user32_message() -> u32 {
    mutate(|instance| {
        instance.memory()?;
        record(
            instance,
            MediaRecordKind::WindowsWait,
            0,
            [instance.waiting_user32_message().unwrap_or(0), 0, 0, 0, 0],
        );
        Ok(())
    })
}
pub(crate) fn resume_user32_wait(
    low: u32,
    high: u32,
    continuation_low: u32,
    continuation_high: u32,
) -> u32 {
    mutate(|instance| {
        instance.resume_user32_wait(wide(low, high), wide(continuation_low, continuation_high))
    })
}
