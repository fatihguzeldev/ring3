use super::ProcessStartup32;

#[derive(Debug, PartialEq)]
pub(in crate::process) struct StartupAllocationSnapshot32 {
    bump: usize,
    revision: u64,
    free: Vec<(usize, usize)>,
    directory: (u32, usize),
}
impl ProcessStartup32 {
    pub(in crate::process) fn allocation_snapshot_for_test(&self) -> StartupAllocationSnapshot32 {
        StartupAllocationSnapshot32 {
            bump: self.bump,
            revision: self.allocation_revision,
            free: self.free_blocks.clone(),
            directory: self.current_directory,
        }
    }
    pub(in crate::process) fn allocation_extent_for_test(&self) -> usize {
        self.bump
    }
}
