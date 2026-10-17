mod calls;
mod continuation;
mod conversion;
mod data;

use super::{User32Context32, callback::Pending32};
use crate::windows::user32::clipboard::ClipboardOwner32;

#[derive(Clone, Copy)]
pub(super) enum Intent32 {
    Empty(ClipboardOwner32),
    Get(u32),
}

#[derive(Clone, Copy)]
pub(super) struct ClipboardCall32 {
    pub intent: Intent32,
    pub thread: u32,
    pub message: Option<[u32; 4]>,
    pub child: Option<u64>,
    pub completed: Option<u64>,
    pub dispatched: bool,
    pub ready: Option<(u32, Option<u32>)>,
}

pub(super) fn owns_render(pending: &[Pending32], child: u64, window: u32, format: u32) -> bool {
    pending.iter().any(|parent| {
        let super::callback::Continuation32::Clipboard(call) = parent.continuation else {
            return false;
        };
        matches!(call.intent, Intent32::Get(_))
            && call.child == Some(child)
            && call.message == Some([window, 0x305, format, 0])
    }) && pending.iter().any(|pending| {
        pending.token == child
            && matches!(
                pending.continuation,
                super::callback::Continuation32::Call { .. }
                    | super::callback::Continuation32::RemoteCall { ready: None }
            )
    })
}

impl User32Context32 {
    pub(in crate::process) fn owns_clipboard_render(
        &self,
        child: u64,
        window: u32,
        format: u32,
    ) -> bool {
        owns_render(&self.pending, child, window, format)
    }
}

#[cfg(test)]
mod tests;
