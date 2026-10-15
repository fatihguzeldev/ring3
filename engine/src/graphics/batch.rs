use super::{BATCH_HEADER_SIZE, BatchId, GraphicsError, ResourceId};

pub(super) struct Commands {
    bytes: Vec<u8>,
    count: u32,
    limit: usize,
    sequence: u64,
    sealed: Option<BatchId>,
}

impl Commands {
    pub fn new(limit: u32) -> Result<Self, GraphicsError> {
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(limit as usize)
            .map_err(|_| GraphicsError::Allocation)?;
        bytes.resize(BATCH_HEADER_SIZE, 0);
        Ok(Self {
            bytes,
            count: 0,
            limit: limit as usize,
            sequence: 0,
            sealed: None,
        })
    }

    pub fn check_append(&self, size: usize) -> Result<(), GraphicsError> {
        if self.sealed.is_some() {
            return Err(GraphicsError::Busy);
        }
        if self
            .bytes
            .len()
            .checked_add(size)
            .is_none_or(|end| end > self.limit)
        {
            return Err(GraphicsError::BatchCapacity);
        }
        Ok(())
    }

    pub fn append(&mut self, opcode: u16, payload_size: usize, write: impl FnOnce(&mut Vec<u8>)) {
        let start = self.bytes.len();
        self.bytes.extend_from_slice(&opcode.to_le_bytes());
        self.bytes.extend_from_slice(&0u16.to_le_bytes());
        self.bytes
            .extend_from_slice(&((payload_size + 8) as u32).to_le_bytes());
        write(&mut self.bytes);
        debug_assert_eq!(self.bytes.len(), start + 8 + payload_size);
        self.count += 1;
    }

    pub fn seal(&mut self, epoch: u64) -> Result<Option<BatchId>, GraphicsError> {
        if let Some(id) = self.sealed {
            return Ok(Some(id));
        }
        if self.count == 0 {
            return Ok(None);
        }
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or(GraphicsError::IdentityExhausted)?;
        let id = BatchId { epoch, sequence };
        let length = self.bytes.len() as u32;
        self.bytes[..BATCH_HEADER_SIZE].fill(0);
        self.bytes[..4].copy_from_slice(b"R3GB");
        self.bytes[4..6].copy_from_slice(&1u16.to_le_bytes());
        self.bytes[6..8].copy_from_slice(&1u16.to_le_bytes());
        self.bytes[8..12].copy_from_slice(&length.to_le_bytes());
        self.bytes[12..16].copy_from_slice(&self.count.to_le_bytes());
        self.bytes[16..24].copy_from_slice(&epoch.to_le_bytes());
        self.bytes[24..32].copy_from_slice(&sequence.to_le_bytes());
        self.sequence = sequence;
        self.sealed = Some(id);
        Ok(Some(id))
    }

    pub fn peek(&self) -> Option<(BatchId, &[u8])> {
        self.sealed.map(|id| (id, self.bytes.as_slice()))
    }

    pub fn copy(&self, output: &mut [u8]) -> Result<usize, GraphicsError> {
        let (_, bytes) = self.peek().ok_or(GraphicsError::EmptyBatch)?;
        if output.len() < bytes.len() {
            return Err(GraphicsError::OutputTooSmall);
        }
        output[..bytes.len()].copy_from_slice(bytes);
        Ok(bytes.len())
    }

    pub fn copy_range(&self, offset: u32, output: &mut [u8]) -> Result<usize, GraphicsError> {
        let (_, bytes) = self.peek().ok_or(GraphicsError::EmptyBatch)?;
        let start = offset as usize;
        let end = start
            .checked_add(output.len())
            .filter(|end| *end <= bytes.len())
            .ok_or(GraphicsError::InvalidRange)?;
        output.copy_from_slice(&bytes[start..end]);
        Ok(output.len())
    }

    pub fn acknowledge(&mut self, id: BatchId) -> Result<(), GraphicsError> {
        if self.sealed != Some(id) {
            return Err(GraphicsError::StaleBatch);
        }
        self.clear();
        Ok(())
    }

    pub fn reset(&mut self) {
        self.clear();
        self.sequence = 0;
    }

    fn clear(&mut self) {
        self.bytes.truncate(BATCH_HEADER_SIZE);
        self.bytes.fill(0);
        self.count = 0;
        self.sealed = None;
    }
}

pub(super) fn resource(bytes: &mut Vec<u8>, id: ResourceId) {
    bytes.extend_from_slice(&id.epoch.to_le_bytes());
    bytes.extend_from_slice(&id.slot.to_le_bytes());
    bytes.extend_from_slice(&id.generation.to_le_bytes());
}

pub(super) fn u32(bytes: &mut Vec<u8>, value: u32) {
    bytes.extend_from_slice(&value.to_le_bytes());
}
pub(super) fn u64(bytes: &mut Vec<u8>, value: u64) {
    bytes.extend_from_slice(&value.to_le_bytes());
}
