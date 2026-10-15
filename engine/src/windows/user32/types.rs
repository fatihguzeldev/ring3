use crate::{
    memory::GuestAddress,
    windows::window::{WindowError32, WindowHandle32},
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Point32 {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect32 {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect32 {
    pub fn extent(self) -> Result<crate::windows::window::WindowExtent32, User32Error32> {
        let width = i64::from(self.right) - i64::from(self.left);
        let height = i64::from(self.bottom) - i64::from(self.top);
        if !(0..=i64::from(i32::MAX)).contains(&width)
            || !(0..=i64::from(i32::MAX)).contains(&height)
        {
            return Err(User32Error32::InvalidRequest);
        }
        Ok(crate::windows::window::WindowExtent32 {
            width: width as u32,
            height: height as u32,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct User32Limits32 {
    pub classes: usize,
    pub windows: usize,
    pub messages: usize,
    pub continuations: usize,
}

impl Default for User32Limits32 {
    fn default() -> Self {
        Self {
            classes: 128,
            windows: 1024,
            messages: 4096,
            continuations: 32,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowClass32 {
    pub atom: u16,
    pub name: Vec<u16>,
    pub instance: u32,
    pub procedure: GuestAddress,
    pub unicode: bool,
    pub style: u32,
    pub extra_bytes: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowData32 {
    pub handle: WindowHandle32,
    pub class_atom: u16,
    pub instance: u32,
    pub style: u32,
    pub ex_style: u32,
    pub user_data: u32,
    pub parent: Option<WindowHandle32>,
    pub menu: u32,
    pub outer: Rect32,
    pub client_origin: Point32,
    pub unicode: bool,
    pub destroying: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Message32 {
    pub window: u32,
    pub message: u32,
    pub wparam: u32,
    pub lparam: u32,
    pub time_ms: u32,
    pub point: Point32,
    pub thread_id: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum User32Error32 {
    InvalidLimits,
    InvalidRequest,
    InvalidToken,
    Busy,
    Capacity,
    Allocation,
    Window(WindowError32),
    Memory(crate::memory::MemoryError),
    Win32(u32),
}

impl From<WindowError32> for User32Error32 {
    fn from(error: WindowError32) -> Self {
        Self::Window(error)
    }
}
impl From<crate::memory::MemoryError> for User32Error32 {
    fn from(error: crate::memory::MemoryError) -> Self {
        Self::Memory(error)
    }
}
