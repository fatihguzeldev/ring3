use super::context::stream_error;
use crate::{
    memory::{AddressSpace, GuestAddress, WordWrite32},
    process::windows::ole32::{
        Ole32Runtime32,
        memory::{com_error, iid},
    },
    windows::{
        com::ComError32,
        ole32::{HResult32, stream::StreamId32},
    },
};
impl Ole32Runtime32 {
    pub(super) fn query_stream(
        &mut self,
        memory: &mut AddressSpace,
        pointer: u32,
        requested: u32,
        out: u32,
    ) -> Result<u32, HResult32> {
        let requested = iid(memory, requested)?;
        let target = match self
            .com
            .query_target(memory, GuestAddress(pointer), requested)
        {
            Ok(target) => Some(target),
            Err(crate::process::windows::objects::GuestComError32::Com(
                ComError32::NoInterface,
            )) => None,
            Err(error) => return Err(com_error(error)),
        };
        if out == 0 {
            return Err(HResult32(0x8003_0009));
        }
        let retain = target.map(|_| GuestAddress(pointer));
        let prepared = self
            .com
            .prepare_lifetime_changes(
                memory,
                retain.as_slice(),
                &[],
                &[],
                &[],
                &[WordWrite32 {
                    address: GuestAddress(out),
                    value: target.map_or(0, |target| target.0),
                }],
            )
            .map_err(com_error)?;
        self.com
            .commit_lifetime_changes(memory, prepared)
            .map_err(com_error)?;
        Ok(if target.is_some() {
            0
        } else {
            HResult32::NO_INTERFACE.0
        })
    }
    pub(in crate::process::windows::ole32) fn release_stream(
        &mut self,
        memory: &mut AddressSpace,
        index: usize,
    ) -> Result<u32, HResult32> {
        let entry = self.streams.entries[index].unwrap();
        let references = self.com.references(entry.pointer).map_err(com_error)?;
        if references > 1 {
            return self
                .com
                .release(memory, entry.pointer)
                .map(|result| result.remaining)
                .map_err(com_error);
        }
        let release = self
            .streams
            .store
            .prepare_release_stream(entry.id)
            .map_err(stream_error)?;
        let result = self.com.release(memory, entry.pointer).map_err(com_error)?;
        if result.destroyed.is_some() {
            self.streams.store.commit_release_stream(release);
            self.streams.entries[index] = None;
        }
        Ok(result.remaining)
    }
    pub(in crate::process::windows::ole32) fn stream_id(
        &self,
        memory: &AddressSpace,
        pointer: u32,
    ) -> Result<StreamId32, HResult32> {
        let index = self
            .stream_index(memory, pointer)
            .map_err(|_| HResult32(0x8003_0006))?;
        Ok(self.streams.entries[index].unwrap().id)
    }
}
