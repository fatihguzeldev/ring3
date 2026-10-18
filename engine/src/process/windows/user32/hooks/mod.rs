mod calls;
mod creation;
mod keyboard;
mod state;
use super::{User32Callback32, User32Outcome32};
use crate::{memory::GuestAddress, windows::user32::User32Error32};
pub(in crate::process::windows::user32) use keyboard::{KeyboardMessage32, KeyboardQuery32};
pub(super) use state::Hooks32;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum HookKind32 {
    Keyboard,
    Cbt,
}

#[derive(Clone, Copy)]
pub(super) struct HookCall32 {
    pub kind: HookKind32,
    pub handle: u32,
    pub thread: u32,
    pub procedure: GuestAddress,
    pub arguments: [u32; 3],
}

impl super::User32Context32 {
    pub(super) fn hook_outcome(
        &self,
        call: HookCall32,
        token: u64,
    ) -> Result<User32Outcome32, User32Error32> {
        Ok(User32Outcome32::Callback(User32Callback32 {
            procedure: call.procedure,
            arguments: [call.arguments[0], call.arguments[1], call.arguments[2], 0],
            continuation: token,
            count: 3,
        }))
    }
}

#[cfg(test)]
mod tests;
