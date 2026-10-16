use crate::{
    memory::{GuestAddress, PageRange},
    process::{
        EngineInstance, HostError,
        windows::ole32::{Ole32Runtime32, stream::context::STREAM_PAYLOAD},
    },
    windows::{com::ComIdentity32, ole32::stream::PreparedStreamsRelease32},
};
pub(in crate::process::windows::ole32) struct PreparedStreamCopyExit32 {
    thread: u32,
    ranges: Vec<PageRange>,
    unholds: Vec<GuestAddress>,
}
impl PreparedStreamCopyExit32 {
    pub(in crate::process::windows::ole32) fn ranges(&self) -> &[PageRange] {
        &self.ranges
    }
    pub(in crate::process::windows::ole32) fn unholds(&self) -> &[GuestAddress] {
        &self.unholds
    }
}
pub(in crate::process::windows::ole32) struct PreparedDestroyedStreams32 {
    release: PreparedStreamsRelease32,
    slots: Vec<usize>,
}
impl Ole32Runtime32 {
    pub(in crate::process::windows::ole32) fn prepare_destroyed_streams(
        &self,
        destroyed: &[ComIdentity32],
    ) -> Result<PreparedDestroyedStreams32, HostError> {
        let mut ids = Vec::new();
        let mut slots = Vec::new();
        ids.try_reserve_exact(destroyed.len())
            .map_err(|_| HostError::Infrastructure)?;
        slots
            .try_reserve_exact(destroyed.len())
            .map_err(|_| HostError::Infrastructure)?;
        for identity in destroyed {
            if identity.payload & !(STREAM_PAYLOAD - 1) == STREAM_PAYLOAD {
                let slot = (identity.payload & (STREAM_PAYLOAD - 1)) as usize;
                let entry = self
                    .streams
                    .entries
                    .get(slot)
                    .and_then(Option::as_ref)
                    .ok_or(HostError::Infrastructure)?;
                ids.push(entry.id);
                slots.push(slot);
            }
        }
        let release = self
            .streams
            .store
            .prepare_release_streams(&ids)
            .map_err(|_| HostError::Infrastructure)?;
        Ok(PreparedDestroyedStreams32 { release, slots })
    }
    pub(in crate::process::windows::ole32) fn commit_destroyed_streams(
        &mut self,
        prepared: PreparedDestroyedStreams32,
    ) {
        self.streams.store.commit_release_streams(prepared.release);
        for slot in prepared.slots {
            self.streams.entries[slot] = None;
        }
    }
}
impl EngineInstance {
    pub(in crate::process::windows::ole32) fn prepare_stream_copy_exit(
        &self,
        thread: u32,
    ) -> Result<PreparedStreamCopyExit32, HostError> {
        let mut result = PreparedStreamCopyExit32 {
            thread,
            ranges: Vec::new(),
            unholds: Vec::new(),
        };
        let count = self.ole32.as_ref().map_or(0, |runtime| {
            runtime
                .streams
                .copies
                .iter()
                .filter(|call| call.execution.thread == thread)
                .count()
        });
        result
            .ranges
            .try_reserve_exact(count)
            .map_err(|_| HostError::Infrastructure)?;
        result
            .unholds
            .try_reserve_exact(count)
            .map_err(|_| HostError::Infrastructure)?;
        if let Some(runtime) = &self.ole32 {
            for call in runtime
                .streams
                .copies
                .iter()
                .filter(|call| call.execution.thread == thread)
            {
                // No foreign pointer or caller output is read/written during
                // retirement. Only the engine-owned scratch and source hold
                // must still belong to this exact operation.
                call.scratch
                    .check(self.memory()?)
                    .map_err(HostError::Memory)?;
                let source = runtime.stream_index(self.memory()?, call.source.0)?;
                if runtime.streams.entries[source].unwrap().id != call.id {
                    return Err(HostError::InvalidRequest);
                }
                result.ranges.push(call.scratch.range());
                result.unholds.push(call.source);
            }
        }
        Ok(result)
    }
    pub(in crate::process::windows::ole32) fn commit_stream_copy_exit(
        &mut self,
        prepared: PreparedStreamCopyExit32,
    ) {
        if let Some(runtime) = self.ole32.as_mut() {
            runtime
                .streams
                .copies
                .retain(|call| call.execution.thread != prepared.thread);
        }
    }
}
