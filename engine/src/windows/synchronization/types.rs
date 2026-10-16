pub const EVENT_MODIFY_STATE: u32 = 2;
pub const SYNCHRONIZE: u32 = 0x0010_0000;
pub const EVENT_ALL_ACCESS: u32 = 0x001f_0003;
pub const WAIT_TIMEOUT: u32 = 258;
pub const WAIT_FAILED: u32 = u32::MAX;
pub const INFINITE: u32 = u32::MAX;
pub const MAX_WAIT_OBJECTS: usize = 64;
pub(super) const MAX_EVENTS: usize = 4096;
pub(super) const STATE_WORDS: usize = MAX_EVENTS / 64;
pub(super) const MAX_WAITS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SynchronizationLimits32 {
    pub events: u32,
    pub handles: u32,
    pub bindings: u32,
    pub waits: u32,
}
impl Default for SynchronizationLimits32 {
    fn default() -> Self {
        Self {
            events: 1024,
            handles: 4096,
            bindings: 4096,
            waits: 32,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SynchronizationError32 {
    InvalidLimits,
    InvalidHandle,
    AccessDenied,
    InvalidName,
    NotFound,
    Capacity,
    Allocation,
    Exhausted,
    Stale,
    InvalidWait,
    ClockBackwards,
    DeadlineOverflow,
    Unsupported,
    Busy,
    NotOwner,
    TooManyPosts,
}
impl SynchronizationError32 {
    pub fn win32(self) -> u32 {
        match self {
            Self::InvalidHandle | Self::Stale => 6,
            Self::AccessDenied => 5,
            Self::NotFound => 2,
            Self::InvalidName => 123,
            Self::Capacity | Self::Allocation | Self::Exhausted => 8,
            Self::Unsupported => 50,
            Self::Busy => 170,
            Self::NotOwner => 288,
            Self::TooManyPosts => 298,
            _ => 87,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventBinding32 {
    pub(super) epoch: u64,
    pub(super) id: u64,
    pub(super) slot: usize,
    pub(super) object: u64,
    pub(super) event: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WaitToken32 {
    pub(super) epoch: u64,
    pub(super) id: u64,
    pub(super) slot: usize,
}
impl WaitToken32 {
    pub fn id(self) -> u64 {
        self.id
    }
    pub fn epoch(self) -> u64 {
        self.epoch
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaitMode32 {
    Any,
    All,
    Sleep,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MessageWait32 {
    pub mask: u32,
    pub input_available: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WaitResult32 {
    pub value: u32,
    pub last_error: Option<u32>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaitOutcome32 {
    Ready(WaitResult32),
    Pending(WaitToken32),
}
