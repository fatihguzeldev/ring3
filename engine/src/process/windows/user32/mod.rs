mod dialogs;
mod gdi;
pub use dialogs::{User32DialogIcon32, User32DialogRequest32};
mod accelerators;
mod activation;
mod callback;
mod classes;
mod clipboard;
mod context;
mod controls;
mod destruction;
mod input;
mod lifecycle;
mod memory;
mod menus;
mod messages;
mod properties;
mod rawinput;
mod runtime;
mod threads;
mod timers;
pub(in crate::process) use timers::PreparedUser32TimerClock32;

use crate::{
    memory::{GuestAddress, PageRange},
    windows::{
        user32::{Point32, User32State32},
        window::WindowHandle32,
    },
};
use callback::Pending32;
pub use input::User32CursorRequest32;
pub(in crate::process) use threads::{PreparedUser32Sent32, User32ThreadReply32};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct User32CallContext32 {
    pub thread_id: u32,
    pub instance: u32,
    pub time_ms: u32,
    pub now_ns: u64,
    pub ansi: crate::windows::text::CodePage32,
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
    pub count: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum User32Outcome32 {
    Return(u32),
    Callback(User32Callback32),
    Wait { continuation: u64 },
}

pub(crate) struct User32Context32 {
    state: User32State32,
    menus: crate::windows::user32::menus::MenuStore32,
    menu_popups: menus::PopupContext32,
    clipboard: crate::windows::user32::clipboard::ClipboardState32,
    accelerators: crate::windows::user32::accelerators::AcceleratorStore32,
    builtin_procedures: Option<[crate::memory::GuestAddress; 2]>,
    layouts: Vec<threads::ThreadLayout32>,
    process_layout: Option<(u64, crate::input::keyboard::KeyboardLayout32)>,
    scratch_bounds: PageRange,
    pending: Vec<Pending32>,
    continuation_limit: usize,
    next_token: u64,
    last_error: Option<u32>,
    runtime_input: User32CallContext32,
    input: input::User32Input32,
    raw_input: crate::windows::rawinput::RawInputRuntime32,
    destructions: Vec<destruction::DestroyPlan32>,
    timers: timers::TimerState32,
    dialogs: Vec<dialogs::Dialog32>,
    resource_dialogs: dialogs::resources::ResourceDialogs32,
    call_activation: crate::windows::activation::CapturedActivation32,
    activation_capture_limit: usize,
    provenance: messages::MessageProvenance32,
}

fn raw(handle: Option<WindowHandle32>) -> u32 {
    handle.map_or(0, WindowHandle32::get)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "menus/tests.rs"]
mod menu_tests;
