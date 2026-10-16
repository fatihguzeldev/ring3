use crate::memory::PageRange;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ole32Config32 {
    pub objects: PageRange,
    pub max_objects: u32,
    pub max_threads: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ole32Error32 {
    InvalidConfiguration,
    Allocation,
    NotConfigured,
    AlreadyConfigured,
    Busy,
}
