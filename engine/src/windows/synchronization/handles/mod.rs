use super::{SynchronizationContext32, SynchronizationError32 as Error};
impl SynchronizationContext32 {
    pub fn handle_flags(&self, handle: u32) -> Result<u32, Error> {
        let handle = self.handles[self.handle_index(handle, 0)?];
        Ok(u32::from(handle.inherit) | u32::from(handle.protected) << 1)
    }
    pub fn set_handle_flags(&mut self, handle: u32, mask: u32, flags: u32) -> Result<(), Error> {
        if mask & !3 != 0 {
            return Err(Error::InvalidWait);
        }
        let index = self.handle_index(handle, 0)?;
        let revision = self.next_revision()?;
        let old = self.handle_flags(handle)?;
        let flags = (old & !mask) | (flags & mask);
        self.handles[index].inherit = flags & 1 != 0;
        self.handles[index].protected = flags & 2 != 0;
        self.revision = revision;
        Ok(())
    }
}
