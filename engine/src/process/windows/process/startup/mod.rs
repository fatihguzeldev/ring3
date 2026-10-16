mod allocator;
mod bytes;
mod directory;
pub(super) mod loader;
mod staging;

use crate::{loader::ModuleId32, memory::PageRange, process::EngineInstance};
use loader::LoaderEntry32;

pub(in crate::process) struct ProcessStartup32 {
    pub(in crate::process) range: PageRange,
    pub(in crate::process) peb: u32,
    pub(super) command_line: [u32; 2],
    pub(super) image_path: String,
    bump: usize,
    pub(super) environment_blocks: Vec<super::environment::EnvironmentSnapshot32>,
    pub(super) current_environment: (u32, usize),
    free_blocks: Vec<(usize, usize)>,
    allocation_revision: u64,
    current_directory: (u32, usize),
    loader_entries: Vec<LoaderEntry32>,
    loader_initialization: Vec<ModuleId32>,
}

impl EngineInstance {
    pub fn process_peb_address(&self) -> Option<u32> {
        self.process_runtime
            .startup
            .as_ref()
            .map(|startup| startup.peb)
    }
}

#[cfg(test)]
mod test_support;
