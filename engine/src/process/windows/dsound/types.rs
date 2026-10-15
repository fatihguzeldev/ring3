use crate::memory::PageRange;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DSoundConfig32 {
    pub objects: PageRange,
    pub locks: PageRange,
    pub max_objects: u32,
    pub max_locks: u32,
    pub max_notifications: u32,
    pub max_capture_bytes: u32,
    pub capture_available: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DSoundError32 {
    InvalidConfiguration,
    Allocation,
    NotConfigured,
    AlreadyConfigured,
    InvalidRequest,
}
