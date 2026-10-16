#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThreadLimits32 {
    pub threads: u32,
    pub handles: u32,
    pub apcs: u32,
    pub stack_pages: u32,
    pub stack_reserve_pages: u32,
}
impl Default for ThreadLimits32 {
    fn default() -> Self {
        Self {
            threads: 128,
            handles: 4096,
            apcs: 256,
            stack_pages: 16,
            stack_reserve_pages: 256,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThreadError32 {
    InvalidLimits,
    InvalidHandle,
    InvalidParameter,
    AccessDenied,
    NoAccess,
    Capacity,
    Allocation,
    Exhausted,
    Busy,
    Stale,
    Unsupported,
    NoRunnableThread,
}
impl ThreadError32 {
    pub fn win32(self) -> u32 {
        match self {
            Self::InvalidHandle => 6,
            Self::AccessDenied => 5,
            Self::NoAccess => 998,
            Self::Capacity | Self::Allocation | Self::Exhausted => 8,
            Self::Busy => 170,
            Self::Stale => 6,
            Self::Unsupported => 50,
            Self::NoRunnableThread => 258,
            _ => 87,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThreadState32 {
    Running,
    Runnable,
    Suspended,
    Waiting,
    Terminated,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThreadInfo32 {
    pub id: u32,
    pub state: ThreadState32,
    pub teb: u32,
    pub stack_base: u32,
    pub stack_pages: u32,
    pub suspend_count: u32,
    pub exit_code: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThreadSchedule32 {
    Running { thread: u32 },
    WindowsCallback { thread: u32 },
    CpuEvent { thread: u32 },
    Waiting { deadline_ns: Option<u64> },
    Exited { code: u32 },
}
