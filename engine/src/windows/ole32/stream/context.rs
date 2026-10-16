#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BufferId32(pub u64);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StreamId32(pub u64);
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StreamError32 {
    InvalidParameter,
    InvalidHandle,
    InvalidFunction,
    OutOfMemory,
    MediumFull,
    Stale,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StreamLimits32 {
    pub buffers: usize,
    pub streams: usize,
    pub buffer_bytes: usize,
    pub total_bytes: usize,
}
impl StreamLimits32 {
    pub(crate) fn for_objects(objects: usize) -> Result<Self, StreamError32> {
        Ok(Self {
            buffers: objects.checked_mul(4).ok_or(StreamError32::OutOfMemory)?,
            streams: objects,
            buffer_bytes: 1024 * 1024,
            total_bytes: 16 * 1024 * 1024,
        })
    }
}
pub(super) enum BufferData32 {
    Owned { bytes: Vec<u8>, maximum: usize },
    External,
}
pub(super) struct Buffer32 {
    pub id: BufferId32,
    pub data: BufferData32,
    pub charge: usize,
    pub references: usize,
}
#[derive(Clone, Copy)]
pub(super) struct Stream32 {
    pub id: StreamId32,
    pub buffer: usize,
    pub cursor: u64,
}
pub(crate) struct StreamStore32 {
    pub(super) limits: StreamLimits32,
    pub(super) buffers: Vec<Option<Buffer32>>,
    pub(super) streams: Vec<Option<Stream32>>,
    pub(super) reserved: usize,
    pub(super) next: u64,
    pub(super) revision: u64,
}
impl StreamStore32 {
    pub(crate) fn new(limits: StreamLimits32) -> Result<Self, StreamError32> {
        if limits.streams == 0
            || limits.streams > 65536
            || limits.buffers == 0
            || limits.buffers > 262144
            || limits.buffer_bytes == 0
            || limits.buffer_bytes > u32::MAX as usize
            || limits.total_bytes < limits.buffer_bytes
        {
            return Err(StreamError32::InvalidParameter);
        }
        let mut buffers = Vec::new();
        let mut streams = Vec::new();
        buffers
            .try_reserve_exact(limits.buffers)
            .map_err(|_| StreamError32::OutOfMemory)?;
        streams
            .try_reserve_exact(limits.streams)
            .map_err(|_| StreamError32::OutOfMemory)?;
        buffers.resize_with(limits.buffers, || None);
        streams.resize_with(limits.streams, || None);
        Ok(Self {
            limits,
            buffers,
            streams,
            reserved: 0,
            next: 0,
            revision: 0,
        })
    }
    pub(crate) fn buffer_limit(&self) -> usize {
        self.limits.buffer_bytes
    }
    #[cfg(test)]
    pub(crate) fn reserved_bytes(&self) -> usize {
        self.reserved
    }
    pub(super) fn changed(&self) -> Result<(), StreamError32> {
        self.revision
            .checked_add(1)
            .ok_or(StreamError32::OutOfMemory)
            .map(|_| ())
    }
    pub(super) fn stream(&self, id: StreamId32) -> Result<(usize, Stream32), StreamError32> {
        self.streams
            .iter()
            .enumerate()
            .find_map(|(slot, entry)| {
                entry
                    .filter(|stream| stream.id == id)
                    .map(|stream| (slot, stream))
            })
            .ok_or(StreamError32::InvalidHandle)
    }
    pub(super) fn owned(&self, slot: usize) -> (&[u8], usize) {
        self.buffers[slot].as_ref().unwrap().bytes()
    }
    pub(super) fn check_revision(&self, revision: u64) -> Result<(), StreamError32> {
        if revision == self.revision {
            Ok(())
        } else {
            Err(StreamError32::Stale)
        }
    }
}

impl Buffer32 {
    pub(super) fn bytes(&self) -> (&[u8], usize) {
        match &self.data {
            BufferData32::Owned { bytes, maximum } => (bytes, *maximum),
            BufferData32::External => unreachable!("stream identities retain owned buffers"),
        }
    }
    pub(super) fn bytes_mut(&mut self) -> &mut Vec<u8> {
        match &mut self.data {
            BufferData32::Owned { bytes, .. } => bytes,
            BufferData32::External => unreachable!("stream identities retain owned buffers"),
        }
    }
}
