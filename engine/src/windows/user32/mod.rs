mod api;
mod colors;
mod queue;
mod state;
mod types;

pub(crate) mod dialogs;

pub use api::{User32Api32, user32_module};
pub(crate) use colors::SystemColors32;
pub(crate) use queue::{PreparedSentChange32, QueuedKey32, QueuedKeyboard32};
pub(crate) use state::User32State32;
pub use types::{
    Message32, Point32, Rect32, User32Error32, User32Limits32, WindowClass32, WindowData32,
};

#[cfg(test)]
mod tests;

pub(crate) mod menus;
pub(crate) mod accelerators;
