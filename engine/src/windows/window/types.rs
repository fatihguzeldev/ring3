use crate::memory::GuestAddress;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct WindowHandle32(u32);

impl WindowHandle32 {
    pub fn get(self) -> u32 {
        self.0
    }

    pub(super) fn new(value: u32) -> Self {
        Self(value)
    }
}

impl TryFrom<u32> for WindowHandle32 {
    type Error = WindowError32;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        if value == 0 {
            return Err(WindowError32::InvalidHandle);
        }
        Ok(Self(value))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowExtent32 {
    pub width: u32,
    pub height: u32,
}

impl WindowExtent32 {
    pub(super) fn validate(self) -> Result<(), WindowError32> {
        if self.width > i32::MAX as u32 || self.height > i32::MAX as u32 {
            return Err(WindowError32::InvalidExtent);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowDescription32 {
    pub thread_id: u32,
    pub procedure: GuestAddress,
    pub extent: WindowExtent32,
    pub visible: bool,
    pub enabled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowRecord32 {
    pub handle: WindowHandle32,
    pub description: WindowDescription32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowMessage32 {
    pub window: WindowHandle32,
    pub procedure: GuestAddress,
    pub message: u32,
    pub wparam: u32,
    pub lparam: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowError32 {
    InvalidLimits,
    InvalidHandle,
    InvalidExtent,
    InvalidThread,
    Disabled,
    Capacity,
    QueueFull,
    Exhausted,
    Allocation,
}
