use crate::{
    memory::{Access, GuestAddress},
    process::EngineInstance,
    windows::memory::MemoryError32,
};

impl EngineInstance {
    pub(super) fn initialize_slist_head(&mut self, pointer: u32) -> Result<u32, MemoryError32> {
        if !pointer.is_multiple_of(8) {
            return Err(MemoryError32::InvalidParameter);
        }
        self.memory
            .as_ref()
            .unwrap()
            .check_span(GuestAddress(pointer), 8, Access::Write)?;
        self.memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(pointer), &[0; 8])?;
        Ok(0)
    }
}
