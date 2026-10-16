use super::{buffers::PreparedBuffer32, context::*};
pub(crate) struct PreparedStream32 {
    revision: u64,
    buffer: PreparedBuffer32,
    slot: usize,
    id: StreamId32,
}
pub(crate) struct PreparedClone32 {
    revision: u64,
    slot: usize,
    stream: Stream32,
}
pub(crate) struct PreparedStreamRelease32 {
    revision: u64,
    slot: usize,
    buffer: usize,
}
impl StreamStore32 {
    pub(crate) fn prepare_stream(
        &self,
        maximum: usize,
        initial: &[u8],
    ) -> Result<PreparedStream32, StreamError32> {
        let slot = self
            .streams
            .iter()
            .position(Option::is_none)
            .ok_or(StreamError32::OutOfMemory)?;
        let buffer = self.prepare_owned_buffer(maximum, initial)?;
        let id = StreamId32(
            buffer
                .buffer
                .id
                .0
                .checked_add(1)
                .ok_or(StreamError32::OutOfMemory)?,
        );
        Ok(PreparedStream32 {
            revision: self.revision,
            buffer,
            slot,
            id,
        })
    }
    pub(crate) fn can_commit_stream(
        &self,
        prepared: &PreparedStream32,
    ) -> Result<(), StreamError32> {
        self.check_revision(prepared.revision)?;
        self.can_commit_buffer(&prepared.buffer)
    }
    pub(crate) fn commit_stream(&mut self, prepared: PreparedStream32) -> StreamId32 {
        self.can_commit_stream(&prepared)
            .expect("stream publication was preflighted");
        let buffer = prepared.buffer.slot;
        let id = prepared.id;
        self.install_buffer(prepared.buffer);
        self.streams[prepared.slot] = Some(Stream32 {
            id,
            buffer,
            cursor: 0,
        });
        self.next = id.0;
        self.revision += 1;
        id
    }
    pub(crate) fn prepare_clone(&self, id: StreamId32) -> Result<PreparedClone32, StreamError32> {
        self.changed()?;
        let (_, stream) = self.stream(id)?;
        let slot = self
            .streams
            .iter()
            .position(Option::is_none)
            .ok_or(StreamError32::OutOfMemory)?;
        self.buffers[stream.buffer]
            .as_ref()
            .unwrap()
            .references
            .checked_add(1)
            .ok_or(StreamError32::OutOfMemory)?;
        let id = StreamId32(self.next.checked_add(1).ok_or(StreamError32::OutOfMemory)?);
        Ok(PreparedClone32 {
            revision: self.revision,
            slot,
            stream: Stream32 { id, ..stream },
        })
    }
    pub(crate) fn can_commit_clone(&self, prepared: &PreparedClone32) -> Result<(), StreamError32> {
        self.check_revision(prepared.revision)
    }
    pub(crate) fn commit_clone(&mut self, prepared: PreparedClone32) -> StreamId32 {
        self.can_commit_clone(&prepared)
            .expect("clone publication was preflighted");
        self.buffers[prepared.stream.buffer]
            .as_mut()
            .unwrap()
            .references += 1;
        let id = prepared.stream.id;
        self.streams[prepared.slot] = Some(prepared.stream);
        self.next = id.0;
        self.revision += 1;
        id
    }
    pub(crate) fn prepare_release_stream(
        &self,
        id: StreamId32,
    ) -> Result<PreparedStreamRelease32, StreamError32> {
        self.changed()?;
        let (slot, stream) = self.stream(id)?;
        Ok(PreparedStreamRelease32 {
            revision: self.revision,
            slot,
            buffer: stream.buffer,
        })
    }
    pub(crate) fn commit_release_stream(&mut self, prepared: PreparedStreamRelease32) {
        self.check_revision(prepared.revision)
            .expect("stream retirement was preflighted");
        self.streams[prepared.slot] = None;
        self.drop_buffer(prepared.buffer);
        self.revision += 1;
    }
    #[cfg(test)]
    pub(crate) fn position(&self, id: StreamId32) -> Result<u64, StreamError32> {
        Ok(self.stream(id)?.1.cursor)
    }
    pub(crate) fn size(&self, id: StreamId32) -> Result<u64, StreamError32> {
        Ok(self.owned(self.stream(id)?.1.buffer).0.len() as u64)
    }
}

pub(crate) struct PreparedStreamsRelease32 {
    revision: u64,
    slots: Vec<usize>,
}
impl StreamStore32 {
    pub(crate) fn prepare_release_streams(
        &self,
        ids: &[StreamId32],
    ) -> Result<PreparedStreamsRelease32, StreamError32> {
        if !ids.is_empty() {
            self.changed()?;
        }
        let mut slots = Vec::new();
        slots
            .try_reserve_exact(ids.len())
            .map_err(|_| StreamError32::OutOfMemory)?;
        for id in ids {
            let (slot, _) = self.stream(*id)?;
            if slots.contains(&slot) {
                return Err(StreamError32::InvalidParameter);
            }
            slots.push(slot);
        }
        Ok(PreparedStreamsRelease32 {
            revision: self.revision,
            slots,
        })
    }
    pub(crate) fn commit_release_streams(&mut self, prepared: PreparedStreamsRelease32) {
        self.check_revision(prepared.revision)
            .expect("stream retirement was preflighted");
        if !prepared.slots.is_empty() {
            for slot in prepared.slots {
                let stream = self.streams[slot].take().unwrap();
                self.drop_buffer(stream.buffer);
            }
            self.revision += 1;
        }
    }
}
