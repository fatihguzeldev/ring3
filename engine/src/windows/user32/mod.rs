mod api;
mod queue;
mod state;
mod types;

pub use api::{User32Api32, user32_module};
pub(crate) use queue::{QueuedKey32, QueuedKeyboard32};
pub(crate) use state::User32State32;
pub use types::{
    Message32, Point32, Rect32, User32Error32, User32Limits32, WindowClass32, WindowData32,
};

#[cfg(test)]
mod tests;
