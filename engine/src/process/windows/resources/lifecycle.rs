use super::{ResourceError32 as E, ResourceRuntime32};
use crate::memory::{AddressSpace, PageRange};
impl ResourceRuntime32 {
    pub(in crate::process) fn thread_exit_ranges(
        &self,
        memory: &AddressSpace,
        thread: u32,
    ) -> Result<Vec<PageRange>, E> {
        let mut ranges = Vec::new();
        for pending in self
            .pending
            .iter()
            .flatten()
            .filter(|pending| pending.thread == thread)
        {
            if let Some(scratch) = pending.scratch {
                scratch.check(memory)?;
                ranges.try_reserve(1).map_err(|_| E::Allocation)?;
                ranges.push(scratch.range());
            }
        }
        Ok(ranges)
    }
    pub(in crate::process) fn commit_thread_exit(&mut self, thread: u32) {
        for pending in &mut self.pending {
            if pending
                .as_ref()
                .is_some_and(|pending| pending.thread == thread)
            {
                self.enum_bytes -= pending.as_ref().unwrap().charged;
                *pending = None;
            }
        }
    }
    pub(in crate::process) fn commit_process_exit(&mut self) {
        self.pending.iter_mut().for_each(|pending| *pending = None);
        self.handles.fill(None);
        self.enum_bytes = 0;
    }
}
