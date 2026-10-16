use super::context::*;
#[derive(Clone, Copy)]
pub(crate) struct PreparedRead32 {
    revision: u64,
    slot: usize,
    buffer: usize,
    start: usize,
    count: usize,
    cursor: u64,
}
impl PreparedRead32 {
    pub(crate) fn length(self) -> usize {
        self.count
    }
}
#[cfg(test)]
pub(crate) struct PreparedWrite32<'a> {
    region: PreparedWriteRegion32,
    bytes: &'a [u8],
}
pub(crate) struct PreparedWriteRegion32 {
    revision: u64,
    slot: usize,
    buffer: usize,
    start: usize,
    size: usize,
    cursor: u64,
    length: usize,
}
pub(crate) struct PreparedResize32 {
    revision: u64,
    buffer: usize,
    size: usize,
}
pub(crate) struct PreparedSeek32 {
    revision: u64,
    slot: usize,
    position: u64,
}
impl PreparedSeek32 {
    pub(crate) fn position(&self) -> u64 {
        self.position
    }
}
pub(crate) struct PreparedCopy32 {
    revision: u64,
    source: usize,
    destination: usize,
    source_buffer: usize,
    destination_buffer: usize,
    start: usize,
    target: usize,
    count: usize,
    size: usize,
    source_cursor: u64,
    destination_cursor: u64,
}
impl PreparedCopy32 {
    pub(crate) fn length(&self) -> usize {
        self.count
    }
}
impl StreamStore32 {
    pub(crate) fn prepare_read(
        &self,
        id: StreamId32,
        count: usize,
    ) -> Result<PreparedRead32, StreamError32> {
        self.changed()?;
        let (slot, stream) = self.stream(id)?;
        let bytes = self.owned(stream.buffer).0;
        let start = usize::try_from(stream.cursor)
            .unwrap_or(bytes.len())
            .min(bytes.len());
        let count = count.min(bytes.len() - start);
        Ok(PreparedRead32 {
            revision: self.revision,
            slot,
            buffer: stream.buffer,
            start,
            count,
            cursor: stream.cursor + count as u64,
        })
    }
    pub(crate) fn read_bytes(&self, prepared: &PreparedRead32) -> Result<&[u8], StreamError32> {
        self.check_revision(prepared.revision)?;
        Ok(&self.owned(prepared.buffer).0[prepared.start..prepared.start + prepared.count])
    }
    pub(crate) fn commit_read(&mut self, prepared: PreparedRead32) {
        self.check_revision(prepared.revision)
            .expect("read outputs were preflighted");
        self.streams[prepared.slot].as_mut().unwrap().cursor = prepared.cursor;
        self.revision += 1;
    }
    #[cfg(test)]
    pub(crate) fn prepare_write<'a>(
        &self,
        id: StreamId32,
        bytes: &'a [u8],
    ) -> Result<PreparedWrite32<'a>, StreamError32> {
        Ok(PreparedWrite32 {
            region: self.prepare_write_region(id, bytes.len())?,
            bytes,
        })
    }
    pub(crate) fn prepare_write_region(
        &self,
        id: StreamId32,
        length: usize,
    ) -> Result<PreparedWriteRegion32, StreamError32> {
        self.changed()?;
        let (slot, stream) = self.stream(id)?;
        let (old, maximum) = self.owned(stream.buffer);
        let cursor = stream
            .cursor
            .checked_add(length as u64)
            .ok_or(StreamError32::MediumFull)?;
        let (start, size) = if length == 0 {
            (0, old.len())
        } else {
            let start = usize::try_from(stream.cursor).map_err(|_| StreamError32::MediumFull)?;
            let end = usize::try_from(cursor).map_err(|_| StreamError32::MediumFull)?;
            if end > maximum {
                return Err(StreamError32::MediumFull);
            }
            (start, end.max(old.len()))
        };
        Ok(PreparedWriteRegion32 {
            revision: self.revision,
            slot,
            buffer: stream.buffer,
            start,
            size,
            cursor,
            length,
        })
    }
    #[cfg(test)]
    pub(crate) fn can_commit_write(
        &self,
        prepared: &PreparedWrite32<'_>,
    ) -> Result<(), StreamError32> {
        self.check_revision(prepared.region.revision)
    }
    #[cfg(test)]
    pub(crate) fn commit_write(&mut self, prepared: PreparedWrite32<'_>) {
        self.can_commit_write(&prepared)
            .expect("write outputs and version were preflighted");
        self.commit_write_region(prepared.region, |output| {
            output.copy_from_slice(prepared.bytes)
        });
    }
    pub(crate) fn commit_write_region(
        &mut self,
        prepared: PreparedWriteRegion32,
        fill: impl FnOnce(&mut [u8]),
    ) {
        self.check_revision(prepared.revision)
            .expect("write inputs and outputs were preflighted");
        let bytes = self.buffers[prepared.buffer].as_mut().unwrap().bytes_mut();
        assert!(prepared.size <= bytes.capacity());
        bytes.resize(prepared.size, 0);
        fill(&mut bytes[prepared.start..prepared.start + prepared.length]);
        self.streams[prepared.slot].as_mut().unwrap().cursor = prepared.cursor;
        self.revision += 1;
    }
    pub(crate) fn prepare_seek(
        &self,
        id: StreamId32,
        movement: u64,
        origin: u32,
    ) -> Result<PreparedSeek32, StreamError32> {
        self.changed()?;
        let (slot, stream) = self.stream(id)?;
        let position = match origin {
            0 => movement,
            1 | 2 => {
                let base = if origin == 1 {
                    stream.cursor
                } else {
                    self.owned(stream.buffer).0.len() as u64
                };
                u64::try_from(i128::from(base) + i128::from(movement as i64))
                    .map_err(|_| StreamError32::InvalidFunction)?
            }
            _ => return Err(StreamError32::InvalidFunction),
        };
        Ok(PreparedSeek32 {
            revision: self.revision,
            slot,
            position,
        })
    }
    pub(crate) fn commit_seek(&mut self, prepared: PreparedSeek32) {
        self.check_revision(prepared.revision)
            .expect("seek output was preflighted");
        self.streams[prepared.slot].as_mut().unwrap().cursor = prepared.position;
        self.revision += 1;
    }
    pub(crate) fn prepare_resize(
        &self,
        id: StreamId32,
        size: u64,
    ) -> Result<PreparedResize32, StreamError32> {
        self.changed()?;
        let (_, stream) = self.stream(id)?;
        let size = usize::try_from(size).map_err(|_| StreamError32::MediumFull)?;
        if size > self.owned(stream.buffer).1 {
            return Err(StreamError32::MediumFull);
        }
        Ok(PreparedResize32 {
            revision: self.revision,
            buffer: stream.buffer,
            size,
        })
    }
    pub(crate) fn commit_resize(&mut self, prepared: PreparedResize32) {
        self.check_revision(prepared.revision)
            .expect("resize was preflighted");
        let bytes = self.buffers[prepared.buffer].as_mut().unwrap().bytes_mut();
        assert!(prepared.size <= bytes.capacity());
        bytes.resize(prepared.size, 0);
        self.revision += 1;
    }
    pub(crate) fn prepare_copy(
        &self,
        source: StreamId32,
        destination: StreamId32,
        count: u64,
    ) -> Result<PreparedCopy32, StreamError32> {
        let read = self.prepare_read(source, usize::try_from(count).unwrap_or(usize::MAX))?;
        let (destination, target) = self.stream(destination)?;
        let (old, maximum) = self.owned(target.buffer);
        let target_cursor = if read.slot == destination {
            read.cursor
        } else {
            target.cursor
        };
        let destination_cursor = target_cursor
            .checked_add(read.count as u64)
            .ok_or(StreamError32::MediumFull)?;
        let (target_offset, size) = if read.count == 0 {
            (0, old.len())
        } else {
            let offset = usize::try_from(target_cursor).map_err(|_| StreamError32::MediumFull)?;
            let end = usize::try_from(destination_cursor).map_err(|_| StreamError32::MediumFull)?;
            if end > maximum {
                return Err(StreamError32::MediumFull);
            }
            (offset, end.max(old.len()))
        };
        Ok(PreparedCopy32 {
            revision: self.revision,
            source: read.slot,
            destination,
            source_buffer: read.buffer,
            destination_buffer: target.buffer,
            start: read.start,
            target: target_offset,
            count: read.count,
            size,
            source_cursor: read.cursor,
            destination_cursor,
        })
    }
    pub(crate) fn commit_copy(&mut self, prepared: PreparedCopy32) {
        self.check_revision(prepared.revision)
            .expect("copy outputs were preflighted");
        let start = prepared.start;
        let end = start + prepared.count;
        if prepared.source_buffer == prepared.destination_buffer {
            let bytes = self.buffers[prepared.source_buffer]
                .as_mut()
                .unwrap()
                .bytes_mut();
            bytes.resize(prepared.size, 0);
            bytes.copy_within(start..end, prepared.target);
        } else {
            let (source, destination) = if prepared.source_buffer < prepared.destination_buffer {
                let (left, right) = self.buffers.split_at_mut(prepared.destination_buffer);
                (
                    left[prepared.source_buffer].as_ref().unwrap(),
                    right[0].as_mut().unwrap(),
                )
            } else {
                let (left, right) = self.buffers.split_at_mut(prepared.source_buffer);
                (
                    right[0].as_ref().unwrap(),
                    left[prepared.destination_buffer].as_mut().unwrap(),
                )
            };
            let from = source.bytes().0;
            let to = destination.bytes_mut();
            to.resize(prepared.size, 0);
            to[prepared.target..prepared.target + prepared.count]
                .copy_from_slice(&from[start..end]);
        }
        self.streams[prepared.source].as_mut().unwrap().cursor = prepared.source_cursor;
        self.streams[prepared.destination].as_mut().unwrap().cursor = prepared.destination_cursor;
        self.revision += 1;
    }
}
