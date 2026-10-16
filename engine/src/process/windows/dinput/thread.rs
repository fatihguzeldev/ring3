use super::{DirectInputRuntime32, enumeration::PendingEnumeration32};
pub(in crate::process) struct DirectInputThreadReply32 {
    pending: Vec<PendingEnumeration32>,
}
impl DirectInputRuntime32 {
    pub(in crate::process) fn empty_thread_reply(&self) -> DirectInputThreadReply32 {
        DirectInputThreadReply32 {
            pending: Vec::new(),
        }
    }
    pub(in crate::process) fn take_thread_reply(&mut self) -> DirectInputThreadReply32 {
        DirectInputThreadReply32 {
            pending: std::mem::take(&mut self.pending),
        }
    }
    pub(in crate::process) fn restore_thread_reply(&mut self, reply: DirectInputThreadReply32) {
        assert!(self.pending.is_empty());
        self.pending = reply.pending;
    }
    pub(in crate::process) fn thread_scratch_ranges(
        reply: &DirectInputThreadReply32,
    ) -> impl Iterator<Item = crate::memory::PageRange> + '_ {
        reply.pending.iter().map(|pending| pending.range)
    }
}

impl DirectInputRuntime32 {
    pub(in crate::process) fn check_thread_scratch(
        reply: &DirectInputThreadReply32,
        memory: &crate::memory::AddressSpace,
    ) -> Result<(), crate::process::HostError> {
        for pending in &reply.pending {
            if pending.memory != memory.identity()
                || memory.mapping_version_at(pending.scratch) != Ok(pending.mapping)
            {
                return Err(crate::process::HostError::Thread(
                    crate::windows::thread::ThreadError32::Stale,
                ));
            }
        }
        Ok(())
    }
    pub(in crate::process) fn check_active_thread_scratch(
        &self,
        memory: &crate::memory::AddressSpace,
    ) -> Result<(), crate::process::HostError> {
        for pending in &self.pending {
            if pending.memory != memory.identity()
                || memory.mapping_version_at(pending.scratch) != Ok(pending.mapping)
            {
                return Err(crate::process::HostError::Thread(
                    crate::windows::thread::ThreadError32::Stale,
                ));
            }
        }
        Ok(())
    }
    pub(in crate::process) fn active_thread_scratch_ranges(
        &self,
    ) -> impl Iterator<Item = crate::memory::PageRange> + '_ {
        self.pending.iter().map(|pending| pending.range)
    }
}
