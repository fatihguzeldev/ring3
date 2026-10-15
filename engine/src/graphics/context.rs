use super::{
    BatchId, GraphicsError, GraphicsLimits, ResourceId, VERTEX_STRIDE, VertexFormat,
    batch::{self, Commands},
    resource::{Buffer, Slot, validate_vertices},
};

pub struct GraphicsContext {
    epoch: u64,
    limits: GraphicsLimits,
    slots: Vec<Slot>,
    resource_bytes: usize,
    commands: Commands,
}

impl GraphicsContext {
    pub fn new(epoch: u64, limits: GraphicsLimits) -> Result<Self, GraphicsError> {
        if epoch == 0 {
            return Err(GraphicsError::InvalidEpoch);
        }
        limits.validate()?;
        let mut slots = Vec::new();
        slots
            .try_reserve_exact(limits.resources as usize)
            .map_err(|_| GraphicsError::Allocation)?;
        slots.resize_with(limits.resources as usize, Slot::default);
        Ok(Self {
            epoch,
            limits,
            slots,
            resource_bytes: 0,
            commands: Commands::new(limits.batch_bytes)?,
        })
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }
    pub fn limits(&self) -> GraphicsLimits {
        self.limits
    }
    pub fn resource_bytes(&self) -> usize {
        self.resource_bytes
    }

    pub fn can_reset(&self, epoch: u64) -> Result<(), GraphicsError> {
        if epoch == 0 || epoch <= self.epoch {
            return Err(GraphicsError::InvalidEpoch);
        }
        Ok(())
    }

    pub fn reset(&mut self, epoch: u64) -> Result<(), GraphicsError> {
        self.can_reset(epoch)?;
        for slot in &mut self.slots {
            slot.buffer = None;
            slot.generation = 0;
        }
        self.resource_bytes = 0;
        self.commands.reset();
        self.epoch = epoch;
        Ok(())
    }

    pub fn create_vertex_buffer(&mut self, bytes: &[u8]) -> Result<ResourceId, GraphicsError> {
        self.commands.check_append(0)?;
        let count = validate_vertices(bytes)?;
        let used = self
            .resource_bytes
            .checked_add(bytes.len())
            .filter(|total| *total <= self.limits.resource_bytes as usize)
            .ok_or(GraphicsError::ByteCapacity)?;
        self.commands.check_append(40 + bytes.len())?;
        let index = self
            .slots
            .iter()
            .position(|slot| slot.buffer.is_none() && slot.generation != u32::MAX)
            .ok_or(if self.slots.iter().any(|slot| slot.buffer.is_none()) {
                GraphicsError::IdentityExhausted
            } else {
                GraphicsError::ResourceCapacity
            })?;
        let generation = self.slots[index].generation + 1;
        let mut stored = Vec::new();
        stored
            .try_reserve_exact(bytes.len())
            .map_err(|_| GraphicsError::Allocation)?;
        stored.extend_from_slice(bytes);
        let id = ResourceId {
            epoch: self.epoch,
            slot: index as u32,
            generation,
        };
        self.commands.append(1, 32 + bytes.len(), |output| {
            batch::resource(output, id);
            batch::u64(output, 1);
            batch::u32(output, count);
            batch::u32(output, VertexFormat::ClipPositionColorRgba8 as u32);
            output.extend_from_slice(bytes);
        });
        self.slots[index] = Slot {
            generation,
            buffer: Some(Buffer {
                bytes: stored,
                version: 1,
            }),
        };
        self.resource_bytes = used;
        Ok(id)
    }

    pub fn update_vertex_buffer(
        &mut self,
        id: ResourceId,
        first_vertex: u32,
        bytes: &[u8],
    ) -> Result<(), GraphicsError> {
        self.commands.check_append(0)?;
        let count = validate_vertices(bytes)?;
        let index = self.index(id)?;
        let buffer = self.slots[index].buffer.as_ref().unwrap();
        let start = (first_vertex as usize)
            .checked_mul(VERTEX_STRIDE)
            .ok_or(GraphicsError::InvalidRange)?;
        let end = start
            .checked_add(bytes.len())
            .filter(|end| *end <= buffer.bytes.len())
            .ok_or(GraphicsError::InvalidRange)?;
        let version = buffer
            .version
            .checked_add(1)
            .ok_or(GraphicsError::IdentityExhausted)?;
        self.commands.check_append(40 + bytes.len())?;
        self.commands.append(2, 32 + bytes.len(), |output| {
            batch::resource(output, id);
            batch::u64(output, version);
            batch::u32(output, first_vertex);
            batch::u32(output, count);
            output.extend_from_slice(bytes);
        });
        let buffer = self.slots[index].buffer.as_mut().unwrap();
        buffer.bytes[start..end].copy_from_slice(bytes);
        buffer.version = version;
        Ok(())
    }

    pub fn draw_triangles(
        &mut self,
        id: ResourceId,
        first_vertex: u32,
        vertex_count: u32,
    ) -> Result<(), GraphicsError> {
        self.commands.check_append(0)?;
        let index = self.index(id)?;
        let buffer = self.slots[index].buffer.as_ref().unwrap();
        let total = buffer.bytes.len() / VERTEX_STRIDE;
        if vertex_count == 0
            || !vertex_count.is_multiple_of(3)
            || u64::from(first_vertex) + u64::from(vertex_count) > total as u64
        {
            return Err(GraphicsError::InvalidRange);
        }
        let version = buffer.version;
        self.commands.check_append(40)?;
        self.commands.append(3, 32, |output| {
            batch::resource(output, id);
            batch::u64(output, version);
            batch::u32(output, first_vertex);
            batch::u32(output, vertex_count);
        });
        Ok(())
    }

    pub fn destroy_vertex_buffer(&mut self, id: ResourceId) -> Result<(), GraphicsError> {
        self.commands.check_append(0)?;
        let index = self.index(id)?;
        self.commands.check_append(24)?;
        self.commands
            .append(4, 16, |output| batch::resource(output, id));
        self.resource_bytes -= self.slots[index].buffer.take().unwrap().bytes.len();
        Ok(())
    }

    pub fn clear(&mut self, color: [f32; 4]) -> Result<(), GraphicsError> {
        self.commands.check_append(0)?;
        if color
            .iter()
            .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
        {
            return Err(GraphicsError::InvalidColor);
        }
        self.commands.check_append(24)?;
        self.commands.append(5, 16, |output| {
            for value in color {
                batch::u32(output, value.to_bits());
            }
        });
        Ok(())
    }

    pub fn present(&mut self, surface: u32) -> Result<(), GraphicsError> {
        self.commands.check_append(0)?;
        if surface == 0 {
            return Err(GraphicsError::InvalidSurface);
        }
        self.commands.check_append(16)?;
        self.commands.append(6, 8, |output| {
            batch::u32(output, surface);
            batch::u32(output, 0);
        });
        Ok(())
    }

    pub fn seal_batch(&mut self) -> Result<Option<BatchId>, GraphicsError> {
        self.commands.seal(self.epoch)
    }
    pub fn peek_batch(&self) -> Option<(BatchId, &[u8])> {
        self.commands.peek()
    }
    pub fn copy_batch(&self, output: &mut [u8]) -> Result<usize, GraphicsError> {
        self.commands.copy(output)
    }
    pub fn copy_batch_range(&self, offset: u32, output: &mut [u8]) -> Result<usize, GraphicsError> {
        self.commands.copy_range(offset, output)
    }
    pub fn acknowledge_batch(&mut self, id: BatchId) -> Result<(), GraphicsError> {
        self.commands.acknowledge(id)
    }

    fn index(&self, id: ResourceId) -> Result<usize, GraphicsError> {
        let index = id.slot as usize;
        if id.epoch != self.epoch
            || id.generation == 0
            || !self
                .slots
                .get(index)
                .is_some_and(|slot| slot.generation == id.generation && slot.buffer.is_some())
        {
            return Err(GraphicsError::StaleResource);
        }
        Ok(index)
    }
}
