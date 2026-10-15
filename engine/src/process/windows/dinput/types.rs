use crate::{
    input::InputError,
    memory::{MemoryError, PageRange},
    windows::synchronization::SynchronizationError32,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirectInputConfig32 {
    pub objects: PageRange,
    pub scratch: PageRange,
    pub max_objects: u32,
    pub max_devices: u32,
    pub max_buffer_events: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectInputError32 {
    NotConfigured,
    AlreadyConfigured,
    InvalidConfiguration,
    InvalidRequest,
    Allocation,
    Busy,
    InvalidToken,
    Memory(MemoryError),
    Input(InputError),
    Synchronization(SynchronizationError32),
}
