mod activation;
mod callback;
mod classes;
mod context;
mod destruction;
mod input;
mod lifecycle;
mod memory;
mod messages;
mod properties;

use crate::{
    memory::{GuestAddress, PageRange},
    windows::{
        user32::{Point32, User32State32},
        window::WindowHandle32,
    },
};
use callback::Pending32;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct User32CallContext32 {
    pub thread_id: u32,
    pub instance: u32,
    pub time_ms: u32,
    pub cursor: Point32,
    pub shift: bool,
    pub caps_lock: bool,
    pub control: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct User32Callback32 {
    pub procedure: GuestAddress,
    pub arguments: [u32; 4],
    pub continuation: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum User32Outcome32 {
    Return(u32),
    Callback(User32Callback32),
    Wait { continuation: u64 },
}

pub(crate) struct User32Context32 {
    state: User32State32,
    scratch_bounds: PageRange,
    pending: Vec<Pending32>,
    continuation_limit: usize,
    next_token: u64,
    last_error: Option<u32>,
    runtime_input: User32CallContext32,
    destructions: Vec<destruction::DestroyPlan32>,
}

fn raw(handle: Option<WindowHandle32>) -> u32 {
    handle.map_or(0, WindowHandle32::get)
}

#[cfg(test)]
mod tests;
