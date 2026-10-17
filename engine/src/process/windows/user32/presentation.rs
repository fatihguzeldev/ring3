use super::User32Context32;
use crate::{
    process::{EngineInstance, HostError},
    windows::{
        user32::{Point32, Rect32, User32Error32},
        window::{WindowError32, WindowExtent32, WindowHandle32},
    },
};

/// the current engine-owned geometry and state of a window available to the host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct User32WindowInfo32 {
    pub window: u32,
    /// the child parent; zero for a top-level window.
    pub parent: u32,
    /// the top-level owner; zero for a child window.
    pub owner: u32,
    pub thread: u32,
    pub style: u32,
    pub ex_style: u32,
    /// outer bounds in desktop coordinates.
    pub outer: Rect32,
    /// client origin in desktop coordinates, including for child windows.
    pub client_origin: Point32,
    pub client_extent: WindowExtent32,
    /// visibility through the child parent chain, as with IsWindowVisible.
    pub visible: bool,
    /// this window's enabled state, as with IsWindowEnabled.
    pub enabled: bool,
}

impl EngineInstance {
    pub fn user32_window_info(&self, window: u32) -> Result<Option<User32WindowInfo32>, HostError> {
        self.memory()?;
        self.user32
            .as_ref()
            .ok_or(HostError::User32(User32Error32::InvalidRequest))?
            .window_info(window)
            .map_err(HostError::User32)
    }
}

impl User32Context32 {
    fn window_info(&self, raw: u32) -> Result<Option<User32WindowInfo32>, User32Error32> {
        let Ok(handle) = WindowHandle32::try_from(raw) else {
            return Ok(None);
        };
        let description = match self.windows().get(handle) {
            Ok(window) => window.description,
            Err(WindowError32::InvalidHandle) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        let data = self.state.record(handle)?;
        if data.destroying {
            return Ok(None);
        }
        let relationship = data.parent.map_or(0, WindowHandle32::get);
        let child = data.style & 0x4000_0000 != 0;
        Ok(Some(User32WindowInfo32 {
            window: raw,
            parent: if child { relationship } else { 0 },
            owner: if child { 0 } else { relationship },
            thread: description.thread_id,
            style: data.style,
            ex_style: data.ex_style,
            outer: data.outer,
            client_origin: data.client_origin,
            client_extent: description.extent,
            visible: self.state.window_visible(handle),
            enabled: description.enabled,
        }))
    }
}

#[cfg(test)]
#[path = "presentation/tests.rs"]
mod tests;
