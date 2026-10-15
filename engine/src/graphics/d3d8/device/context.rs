use super::super::{
    commands::{
        Command, CommandQueue, PipelineId, Progress, ReadbackId, ReadbackKind, Reset,
        ResourceVersion, SurfaceVersion,
    },
    resources::{Pool, ResourceDesc, ResourceStore, SurfaceDesc, SurfaceRef, Usage},
    shader::{
        Fvf,
        registry::{PixelShader, ShaderRegistry, VertexShader},
    },
    state::{DeviceState, StateBlock},
};
use super::{DeviceDescription, DeviceError, DeviceLimits, DeviceStatus};
use crate::graphics::{BatchId, ResourceId};
use std::sync::Arc;

pub struct Device {
    pub(super) description: DeviceDescription,
    pub(super) limits: DeviceLimits,
    pub(super) resources: ResourceStore,
    pub(super) state: DeviceState,
    pub(super) shaders: ShaderRegistry,
    pub(super) queue: CommandQueue,
    pub(super) status: DeviceStatus,
    pub(super) generation: u64,
    pub(super) scene: bool,
    pub(super) back_buffers: Vec<SurfaceRef>,
    pub(super) implicit_depth: Option<SurfaceRef>,
    pub(super) color_target: SurfaceRef,
    pub(super) depth_target: Option<SurfaceRef>,
    pub(super) vertex: Option<Arc<VertexShader>>,
    pub(super) fvf: Option<Arc<Fvf>>,
    pub(super) pixel: Option<Arc<PixelShader>>,
    pub(super) recorded_vertex: Option<Option<Arc<VertexShader>>>,
    pub(super) recorded_fvf: Option<Option<Arc<Fvf>>>,
    pub(super) recorded_pixel: Option<Option<Arc<PixelShader>>>,
    pub(super) blocks: Vec<CapturedBlock>,
    pub(super) retired_shaders: Vec<RetiredShader>,
    pub(super) block_sequence: u32,
    pub(super) block_revision: u64,
    pub(super) pipeline_sequence: u64,
    pub(super) emitted_pipeline: Option<(u64, PipelineId)>,
    pub(super) readbacks: Vec<PendingReadback>,
    pub(super) readback_sequence: u64,
    pub(super) cursor_visible: bool,
    pub(super) gamma: [[u16; 256]; 3],
    pub(super) palettes: Vec<(u32, [[u8; 4]; 256])>,
    pub(super) presented: Option<SurfaceVersion>,
}

pub(super) struct CapturedBlock {
    pub handle: u32,
    pub state: StateBlock,
    pub vertex: Option<Arc<VertexShader>>,
    pub fvf: Option<Arc<Fvf>>,
    pub pixel: Option<Arc<PixelShader>>,
}

pub(super) enum RetiredShader {
    Vertex(u32, Arc<VertexShader>),
    Pixel(u32, Arc<PixelShader>),
}
impl RetiredShader {
    pub fn handle(&self) -> u32 {
        match self {
            Self::Vertex(handle, _) | Self::Pixel(handle, _) => *handle,
        }
    }
    pub fn ready(&self) -> bool {
        match self {
            Self::Vertex(_, shader) => Arc::strong_count(shader) == 1,
            Self::Pixel(_, shader) => Arc::strong_count(shader) == 1,
        }
    }
}

pub(super) struct PendingReadback {
    pub id: ReadbackId,
    pub generation: u64,
    pub source: SurfaceVersion,
    pub destination: SurfaceVersion,
    pub kind: ReadbackKind,
}

impl Device {
    pub fn new(
        epoch: u64,
        description: DeviceDescription,
        limits: DeviceLimits,
    ) -> Result<Self, DeviceError> {
        description.validate()?;
        if limits.state_blocks == 0
            || limits.state_blocks > 4096
            || limits.readbacks == 0
            || limits.readbacks > 1024
        {
            return Err(DeviceError::InvalidCall);
        }
        let mut resources = ResourceStore::new(epoch, limits.graphics)?;
        let mut back_buffers = Vec::new();
        back_buffers
            .try_reserve_exact(description.back_buffers as usize)
            .map_err(|_| DeviceError::Allocation)?;
        for _ in 0..description.back_buffers {
            let id = resources.create(color_description(description))?;
            back_buffers.push(SurfaceRef {
                resource: id,
                subresource: 0,
            });
        }
        let implicit_depth = if let Some(format) = description.depth {
            Some(SurfaceRef {
                resource: resources.create(depth_description(description, format))?,
                subresource: 0,
            })
        } else {
            None
        };
        let mut queue = CommandQueue::new(epoch, limits.commands)?;
        let mut infos = Vec::new();
        infos
            .try_reserve_exact(back_buffers.len() + usize::from(implicit_depth.is_some()))
            .map_err(|_| DeviceError::Allocation)?;
        for surface in back_buffers.iter().copied().chain(implicit_depth) {
            infos.push([resources.info(surface)?]);
        }
        let mut versions = Vec::new();
        versions
            .try_reserve_exact(back_buffers.len())
            .map_err(|_| DeviceError::Allocation)?;
        versions.extend(back_buffers.iter().map(|surface| SurfaceVersion {
            surface: *surface,
            version: 1,
        }));
        let mut commands = Vec::new();
        commands
            .try_reserve_exact(infos.len() + 1)
            .map_err(|_| DeviceError::Allocation)?;
        for (index, surface) in back_buffers
            .iter()
            .copied()
            .chain(implicit_depth)
            .enumerate()
        {
            commands.push(Command::CreateResource {
                id: surface.resource,
                desc: resources.desc(surface.resource)?,
                subresources: &infos[index],
            });
        }
        commands.push(Command::Reset(Reset {
            generation: 1,
            width: description.width,
            height: description.height,
            format: description.format,
            multisample: description.multisample,
            back_buffers: &versions,
            depth: implicit_depth.map(|surface| SurfaceVersion {
                surface,
                version: 1,
            }),
        }));
        let prepared = queue.prepare(&commands)?;
        queue.commit(prepared)?;
        let mut blocks = Vec::new();
        blocks
            .try_reserve_exact(limits.state_blocks as usize)
            .map_err(|_| DeviceError::Allocation)?;
        let mut readbacks = Vec::new();
        readbacks
            .try_reserve_exact(limits.readbacks as usize)
            .map_err(|_| DeviceError::Allocation)?;
        let mut palettes = Vec::new();
        palettes
            .try_reserve_exact(256)
            .map_err(|_| DeviceError::Allocation)?;
        let mut retired_shaders = Vec::new();
        retired_shaders
            .try_reserve_exact(limits.shaders.objects as usize)
            .map_err(|_| DeviceError::Allocation)?;
        let mut state = DeviceState::new(
            description.width,
            description.height,
            description.depth.is_some(),
        )?;
        state.set_render_state(153, u32::from(description.behavior & 0xe0 == 0x20))?;
        let color_target = back_buffers[0];
        Ok(Self {
            description,
            limits,
            resources,
            state,
            shaders: ShaderRegistry::new(limits.shaders)?,
            queue,
            status: DeviceStatus::Operational,
            generation: 1,
            scene: false,
            back_buffers,
            implicit_depth,
            color_target,
            depth_target: implicit_depth,
            vertex: None,
            fvf: None,
            pixel: None,
            recorded_vertex: None,
            recorded_fvf: None,
            recorded_pixel: None,
            blocks,
            retired_shaders,
            block_sequence: 0,
            block_revision: 0,
            pipeline_sequence: 0,
            emitted_pipeline: None,
            readbacks,
            readback_sequence: 0,
            cursor_visible: false,
            gamma: std::array::from_fn(|_| std::array::from_fn(|index| index as u16 * 257)),
            palettes,
            presented: None,
        })
    }

    pub fn description(&self) -> DeviceDescription {
        self.description
    }
    pub fn status(&self) -> DeviceStatus {
        self.status
    }
    pub fn generation(&self) -> u64 {
        self.generation
    }
    pub fn resources(&self) -> &ResourceStore {
        &self.resources
    }
    pub fn state(&self) -> &DeviceState {
        &self.state
    }
    pub fn shaders(&self) -> &ShaderRegistry {
        &self.shaders
    }
    pub fn queue(&self) -> &CommandQueue {
        &self.queue
    }
    pub fn back_buffer(&self, index: u32) -> Result<SurfaceRef, DeviceError> {
        self.back_buffers
            .get(index as usize)
            .copied()
            .ok_or(DeviceError::InvalidCall)
    }
    pub fn depth_surface(&self) -> Option<SurfaceRef> {
        self.implicit_depth
    }
    pub fn render_target(&self) -> SurfaceRef {
        self.color_target
    }
    pub fn depth_target(&self) -> Option<SurfaceRef> {
        self.depth_target
    }
    pub fn seal_batch(&mut self) -> Result<Option<BatchId>, DeviceError> {
        if self.status != DeviceStatus::Removed
            && !self.queue.is_sealed()
            && !self.queue.is_draining()
        {
            self.flush_retired_shaders()?;
        }
        Ok(self.queue.seal()?)
    }
    pub fn acknowledge_batch(&mut self, id: BatchId) -> Result<Progress, DeviceError> {
        Ok(self.queue.acknowledge(id)?)
    }
    pub(crate) fn state_mut(&mut self) -> Result<&mut DeviceState, DeviceError> {
        self.available()?;
        Ok(&mut self.state)
    }
    pub(crate) fn resources_mut(&mut self) -> Result<&mut ResourceStore, DeviceError> {
        self.available()?;
        Ok(&mut self.resources)
    }

    pub(crate) fn metadata_mut(&mut self) -> &mut ResourceStore {
        &mut self.resources
    }

    pub(super) fn available(&self) -> Result<(), DeviceError> {
        match self.status {
            DeviceStatus::Operational => Ok(()),
            DeviceStatus::Lost => Err(DeviceError::Lost),
            DeviceStatus::Resettable => Err(DeviceError::NotReset),
            DeviceStatus::Removed => Err(DeviceError::Removed),
        }
    }

    pub(super) fn resource_version(&self, id: ResourceId) -> Result<ResourceVersion, DeviceError> {
        Ok(ResourceVersion {
            id,
            version: self.resources.version(id)?,
        })
    }
    pub(super) fn surface_version(
        &self,
        surface: SurfaceRef,
    ) -> Result<SurfaceVersion, DeviceError> {
        self.resources.info(surface)?;
        Ok(SurfaceVersion {
            surface,
            version: self.resources.version(surface.resource)?,
        })
    }
}

pub(super) fn color_description(description: DeviceDescription) -> ResourceDesc {
    ResourceDesc::Surface(SurfaceDesc {
        width: description.width,
        height: description.height,
        format: description.format,
        usage: Usage(Usage::RENDER_TARGET),
        pool: Pool::Default,
        multisample: description.multisample,
        lockable: description.lockable_back_buffer,
    })
}

pub(super) fn depth_description(
    description: DeviceDescription,
    format: super::super::resources::Format,
) -> ResourceDesc {
    ResourceDesc::Surface(SurfaceDesc {
        width: description.width,
        height: description.height,
        format,
        usage: Usage(Usage::DEPTH_STENCIL),
        pool: Pool::Default,
        multisample: description.multisample,
        lockable: format == super::super::resources::Format::D16Lockable,
    })
}
