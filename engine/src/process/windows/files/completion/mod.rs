mod apc;
mod calls;
mod packets;
mod waiting;

use crate::windows::files::completion::CompletionPortId32;
#[derive(Clone, Copy)]
pub(super) struct CompletionWait32 {
    pub(in crate::process::windows::files) port: CompletionPortId32,
    pub(in crate::process::windows::files) thread: u32,
    pub(in crate::process::windows::files) deadline: Option<u64>,
    pub(in crate::process::windows::files) alertable: bool,
    pub(in crate::process::windows::files) interrupted: bool,
}

#[cfg(test)]
mod tests;
