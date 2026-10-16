use super::context::*;
pub(crate) struct PreparedBuffer32 {
    pub(super) revision: u64,
    pub(super) slot: usize,
    pub(super) buffer: Buffer32,
}
impl StreamStore32 {
    fn prepare_buffer(
        &self,
        data: BufferData32,
        charge: usize,
    ) -> Result<PreparedBuffer32, StreamError32> {
        self.changed()?;
        if charge > self.limits.buffer_bytes {
            return Err(StreamError32::MediumFull);
        }
        let reserved = self
            .reserved
            .checked_add(charge)
            .ok_or(StreamError32::OutOfMemory)?;
        if reserved > self.limits.total_bytes {
            return Err(StreamError32::OutOfMemory);
        }
        let slot = self
            .buffers
            .iter()
            .position(Option::is_none)
            .ok_or(StreamError32::OutOfMemory)?;
        let id = BufferId32(self.next.checked_add(1).ok_or(StreamError32::OutOfMemory)?);
        Ok(PreparedBuffer32 {
            revision: self.revision,
            slot,
            buffer: Buffer32 {
                id,
                data,
                charge,
                references: 1,
            },
        })
    }
    #[cfg(test)]
    pub(crate) fn prepare_external_buffer(
        &self,
        charge: usize,
    ) -> Result<PreparedBuffer32, StreamError32> {
        self.prepare_buffer(BufferData32::External, charge)
    }
    pub(super) fn prepare_owned_buffer(
        &self,
        maximum: usize,
        initial: &[u8],
    ) -> Result<PreparedBuffer32, StreamError32> {
        if maximum > self.limits.buffer_bytes || initial.len() > maximum {
            return Err(StreamError32::MediumFull);
        }
        self.changed()?;
        if self
            .reserved
            .checked_add(maximum)
            .is_none_or(|size| size > self.limits.total_bytes)
        {
            return Err(StreamError32::OutOfMemory);
        }
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(maximum)
            .map_err(|_| StreamError32::OutOfMemory)?;
        bytes.extend_from_slice(initial);
        let charge = bytes.capacity();
        self.prepare_buffer(BufferData32::Owned { bytes, maximum }, charge)
    }
    pub(crate) fn can_commit_buffer(
        &self,
        prepared: &PreparedBuffer32,
    ) -> Result<(), StreamError32> {
        self.check_revision(prepared.revision)
    }
    #[cfg(test)]
    pub(crate) fn commit_external_buffer(&mut self, prepared: PreparedBuffer32) -> BufferId32 {
        self.can_commit_buffer(&prepared)
            .expect("buffer publication was preflighted");
        let id = prepared.buffer.id;
        self.install_buffer(prepared);
        self.next = id.0;
        self.revision += 1;
        id
    }
    pub(super) fn install_buffer(&mut self, prepared: PreparedBuffer32) {
        assert!(self.buffers[prepared.slot].is_none());
        self.reserved += prepared.buffer.charge;
        self.buffers[prepared.slot] = Some(prepared.buffer);
    }
    #[cfg(test)]
    pub(crate) fn prepare_release_buffer(
        &self,
        id: BufferId32,
    ) -> Result<PreparedBufferRelease32, StreamError32> {
        self.changed()?;
        let slot = self
            .buffers
            .iter()
            .position(|buffer| {
                buffer.as_ref().is_some_and(|buffer| {
                    buffer.id == id && matches!(buffer.data, BufferData32::External)
                })
            })
            .ok_or(StreamError32::InvalidHandle)?;
        Ok(PreparedBufferRelease32 {
            revision: self.revision,
            slot,
        })
    }
    #[cfg(test)]
    pub(crate) fn commit_release_buffer(&mut self, prepared: PreparedBufferRelease32) {
        self.check_revision(prepared.revision)
            .expect("buffer release was preflighted");
        self.drop_buffer(prepared.slot);
        self.revision += 1;
    }
    pub(super) fn drop_buffer(&mut self, slot: usize) {
        let buffer = self.buffers[slot].as_mut().unwrap();
        buffer.references -= 1;
        if buffer.references == 0 {
            self.reserved -= buffer.charge;
            self.buffers[slot] = None;
        }
    }
}
#[cfg(test)]
pub(crate) struct PreparedBufferRelease32 {
    revision: u64,
    slot: usize,
}
