use super::{record, wide};
use crate::{
    abi::media::MediaRecordKind,
    memory::{GuestAddress, PageRange},
    process::{HostError, wasm::mutate},
    windows::user32::{Message32, Point32, Rect32, User32Limits32},
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
pub(crate) fn user32_window_info(window: u32) -> u32 {
    use crate::abi::{
        arena::TRANSFER_OFFSET,
        windows::user32::{USER32_WINDOW_INFO_SIZE, encode_user32_window_info},
    };
    mutate(|instance| {
        let info = instance.user32_window_info(window)?;
        let mut bytes = [0; USER32_WINDOW_INFO_SIZE];
        encode_user32_window_info(info, &mut bytes).map_err(|_| HostError::Infrastructure)?;
        instance.arena.as_mut().get_mut()
            [TRANSFER_OFFSET..TRANSFER_OFFSET + USER32_WINDOW_INFO_SIZE]
            .copy_from_slice(&bytes);
        Ok(())
    })
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

pub(crate) fn pending_user32_input() -> u32 {
    mutate(|instance| {
        instance.memory()?;
        let epoch = instance.media_epoch().unwrap_or(0);
        let pending = u64::from(instance.pending_user32_input());
        record(
            instance,
            MediaRecordKind::User32Input,
            epoch,
            [pending, 0, 0, 0, 0],
        );
        Ok(())
    })
}

pub(crate) fn pending_user32_cursor() -> u32 {
    mutate(|instance| {
        instance.memory()?;
        let pack = |a: i32, b: i32| u64::from(a as u32) | u64::from(b as u32) << 32;
        let values = if let Some(request) = instance.pending_user32_cursor() {
            let flags = u64::from(request.changes)
                | u64::from(request.visible) << 8
                | u64::from(request.clip.is_some()) << 9
                | u64::from(request.cursor) << 32;
            [
                request.revision,
                flags,
                pack(request.position.x, request.position.y),
                request.clip.map_or(0, |clip| pack(clip.left, clip.top)),
                request.clip.map_or(0, |clip| pack(clip.right, clip.bottom)),
            ]
        } else {
            [0; 5]
        };
        record(instance, MediaRecordKind::User32Cursor, 0, values);
        Ok(())
    })
}

pub(crate) fn acknowledge_user32_cursor(revision_low: u32, revision_high: u32) -> u32 {
    mutate(|instance| instance.acknowledge_user32_cursor(wide(revision_low, revision_high)))
}

pub(crate) fn set_user32_desktop(left: u32, top: u32, right: u32, bottom: u32) -> u32 {
    mutate(|instance| {
        instance.set_user32_desktop(Rect32 {
            left: left as i32,
            top: top as i32,
            right: right as i32,
            bottom: bottom as i32,
        })
    })
}
