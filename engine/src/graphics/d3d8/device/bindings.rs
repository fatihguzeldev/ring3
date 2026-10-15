use super::super::{
    shader::{
        Fvf,
        registry::{PixelShader, VertexShader},
    },
    state::{DeviceState, StreamBinding, TEXTURE_STAGES, VERTEX_STREAMS},
};
use super::context::CapturedBlock;
use super::{Device, DeviceError};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Binding {
    Texture {
        stage: u32,
        handle: u32,
    },
    Stream {
        stream: u32,
        handle: u32,
        stride: u32,
    },
    Indices {
        handle: u32,
        base_vertex: u32,
    },
}

pub struct PreparedBinding {
    generation: u64,
    binding: Binding,
    revision: u64,
    recording_revision: u64,
    recording: bool,
    released: Option<u32>,
    retained: Option<u32>,
}
impl PreparedBinding {
    pub fn released_resource(&self) -> Option<u32> {
        self.released
    }
    pub fn retained_resource(&self) -> Option<u32> {
        self.retained
    }
}

pub struct PreparedBlockChange {
    generation: u64,
    index: usize,
    handle: u32,
    revision: u64,
    block_revision: u64,
    released: Vec<u32>,
    retained: Vec<u32>,
    change: BlockChange,
}
enum BlockChange {
    Capture(Box<[CapturedBlock; 1]>),
    Apply {
        state: Box<[DeviceState; 1]>,
        vertex: Option<Arc<VertexShader>>,
        fvf: Option<Arc<Fvf>>,
        pixel: Option<Arc<PixelShader>>,
    },
    Delete,
}
impl PreparedBlockChange {
    pub fn released_resources(&self) -> &[u32] {
        &self.released
    }
    pub fn retained_resources(&self) -> &[u32] {
        &self.retained
    }
}

impl Device {
    pub fn retained_resource_handles(&self) -> impl Iterator<Item = u32> + '_ {
        self.state
            .snapshot()
            .resource_bindings()
            .chain(self.state.recorded_resources())
            .chain(
                self.blocks
                    .iter()
                    .flat_map(|block| block.state.resource_bindings()),
            )
    }

    pub fn prepare_binding(&self, binding: Binding) -> Result<PreparedBinding, DeviceError> {
        self.available()?;
        let (kind, index, handle) = match binding {
            Binding::Texture { stage, handle } if stage < TEXTURE_STAGES as u32 => {
                (0, stage as usize, handle)
            }
            Binding::Stream {
                stream,
                handle,
                stride,
            } if stream < VERTEX_STREAMS as u32 && (handle == 0 || stride <= 2048) => {
                (1, stream as usize, handle)
            }
            Binding::Indices { handle, .. } => (2, 0, handle),
            _ => return Err(DeviceError::InvalidCall),
        };
        let revision = self.state.revision();
        let recording_revision = self.state.recording_revision();
        if (self.state.is_recording() && recording_revision == u64::MAX)
            || (!self.state.is_recording() && revision == u64::MAX)
        {
            return Err(DeviceError::IdentityExhausted);
        }
        Ok(PreparedBinding {
            generation: self.generation,
            binding,
            revision,
            recording_revision,
            recording: self.state.is_recording(),
            released: self.state.retained_binding(kind, index),
            retained: (handle != 0).then_some(handle),
        })
    }

    pub fn can_commit_binding(&self, prepared: &PreparedBinding) -> Result<(), DeviceError> {
        self.available()?;
        if self.generation != prepared.generation
            || self.state.revision() != prepared.revision
            || self.state.recording_revision() != prepared.recording_revision
            || self.state.is_recording() != prepared.recording
        {
            return Err(DeviceError::InvalidCall);
        }
        Ok(())
    }

    pub fn commit_binding(&mut self, prepared: PreparedBinding) -> Result<(), DeviceError> {
        self.can_commit_binding(&prepared)?;
        match prepared.binding {
            Binding::Texture { stage, handle } => self.state.set_texture(stage, handle)?,
            Binding::Stream {
                stream,
                handle,
                stride,
            } => self.state.set_stream(
                stream,
                StreamBinding {
                    resource: handle,
                    stride,
                },
            )?,
            Binding::Indices {
                handle,
                base_vertex,
            } => self.state.set_indices(handle, base_vertex)?,
        }
        Ok(())
    }

    pub fn prepare_capture_state_block(
        &self,
        handle: u32,
    ) -> Result<PreparedBlockChange, DeviceError> {
        self.available()?;
        if self.state.is_recording() {
            return Err(DeviceError::InvalidCall);
        }
        let index = self.block_index(handle)?;
        let previous = &self.blocks[index];
        let mut state = previous.state.try_clone()?;
        self.state.capture_state_block(&mut state)?;
        let block = CapturedBlock {
            handle,
            vertex: state.vertex_shader().and_then(|_| self.vertex.clone()),
            fvf: state.vertex_shader().and_then(|_| self.fvf.clone()),
            pixel: state.pixel_shader().and_then(|_| self.pixel.clone()),
            state,
        };
        self.prepared_block_change(
            index,
            handles(previous.state.resource_bindings())?,
            handles(block.state.resource_bindings())?,
            BlockChange::Capture(prepared_value(block)?),
        )
    }

    pub fn prepare_apply_state_block(
        &self,
        handle: u32,
    ) -> Result<PreparedBlockChange, DeviceError> {
        self.available()?;
        if self.state.is_recording() {
            return Err(DeviceError::InvalidCall);
        }
        let index = self.block_index(handle)?;
        let block = &self.blocks[index];
        let mut state = self.state.try_clone()?;
        state.apply_state_block(&block.state)?;
        let vertex = if block.state.vertex_shader().is_some() {
            block.vertex.clone()
        } else {
            self.vertex.clone()
        };
        let fvf = if block.state.vertex_shader().is_some() {
            block.fvf.clone()
        } else {
            self.fvf.clone()
        };
        let pixel = if block.state.pixel_shader().is_some() {
            block.pixel.clone()
        } else {
            self.pixel.clone()
        };
        self.prepared_block_change(
            index,
            handles(self.state.snapshot().resource_bindings())?,
            handles(state.snapshot().resource_bindings())?,
            BlockChange::Apply {
                state: prepared_value(state)?,
                vertex,
                fvf,
                pixel,
            },
        )
    }

    pub fn prepare_delete_state_block(
        &self,
        handle: u32,
    ) -> Result<PreparedBlockChange, DeviceError> {
        if self.state.is_recording() {
            return Err(DeviceError::InvalidCall);
        }
        let index = self.block_index(handle)?;
        self.prepared_block_change(
            index,
            handles(self.blocks[index].state.resource_bindings())?,
            Vec::new(),
            BlockChange::Delete,
        )
    }

    fn prepared_block_change(
        &self,
        index: usize,
        released: Vec<u32>,
        retained: Vec<u32>,
        change: BlockChange,
    ) -> Result<PreparedBlockChange, DeviceError> {
        if self.block_revision == u64::MAX {
            return Err(DeviceError::IdentityExhausted);
        }
        Ok(PreparedBlockChange {
            generation: self.generation,
            index,
            handle: self.blocks[index].handle,
            revision: self.state.revision(),
            block_revision: self.block_revision,
            released,
            retained,
            change,
        })
    }

    pub fn can_commit_block_change(
        &self,
        prepared: &PreparedBlockChange,
    ) -> Result<(), DeviceError> {
        if self.generation != prepared.generation
            || self.state.is_recording()
            || self.state.revision() != prepared.revision
            || self.block_revision != prepared.block_revision
            || self
                .blocks
                .get(prepared.index)
                .is_none_or(|block| block.handle != prepared.handle)
        {
            return Err(DeviceError::InvalidCall);
        }
        Ok(())
    }

    pub fn commit_block_change(
        &mut self,
        prepared: PreparedBlockChange,
    ) -> Result<(), DeviceError> {
        self.can_commit_block_change(&prepared)?;
        match prepared.change {
            BlockChange::Capture(block) => {
                let [block] = *block;
                self.blocks[prepared.index] = block;
            }
            BlockChange::Apply {
                state,
                vertex,
                fvf,
                pixel,
            } => {
                let [state] = *state;
                self.state = state;
                self.vertex = vertex;
                self.fvf = fvf;
                self.pixel = pixel;
            }
            BlockChange::Delete => {
                self.blocks.remove(prepared.index);
            }
        }
        self.block_revision += 1;
        Ok(())
    }
}

fn handles(values: impl Iterator<Item = u32>) -> Result<Vec<u32>, DeviceError> {
    let mut handles = Vec::new();
    handles
        .try_reserve_exact(TEXTURE_STAGES + VERTEX_STREAMS + 1)
        .map_err(|_| DeviceError::Allocation)?;
    handles.extend(values);
    Ok(handles)
}

fn prepared_value<T>(value: T) -> Result<Box<[T; 1]>, DeviceError> {
    let mut storage = Vec::new();
    storage
        .try_reserve_exact(1)
        .map_err(|_| DeviceError::Allocation)?;
    storage.push(value);
    Ok(storage
        .into_boxed_slice()
        .try_into()
        .unwrap_or_else(|_| unreachable!("one prepared value")))
}
