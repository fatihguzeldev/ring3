use super::super::{
    commands::{Command, PreparedGroup, ShaderDefinition},
    resources::{PreparedReplacement, PreparedResource, ResourceDesc},
    shader::registry::PreparedShader,
    state::StateBlockKind,
};
use super::context::CapturedBlock;
use super::{Device, DeviceError, Outcome};
use crate::graphics::ResourceId;

pub struct PreparedDeviceResource {
    resource: PreparedResource,
    commands: PreparedGroup,
}
impl PreparedDeviceResource {
    pub fn id(&self) -> ResourceId {
        self.resource.id()
    }
}
pub struct PreparedResources {
    resources: PreparedReplacement,
    commands: PreparedGroup,
}
impl PreparedResources {
    pub fn ids(&self) -> &[ResourceId] {
        self.resources.ids()
    }
}

pub struct PreparedDeviceShader {
    shader: PreparedShader,
    commands: PreparedGroup,
}
impl PreparedDeviceShader {
    pub fn handle(&self) -> u32 {
        self.shader.handle()
    }
}

pub struct PreparedStateBlock {
    generation: u64,
    block: CapturedBlock,
    revision: u64,
    recording_revision: u64,
    end_recording: bool,
}
impl PreparedStateBlock {
    pub fn handle(&self) -> u32 {
        self.block.handle
    }
    pub fn resources(&self) -> impl Iterator<Item = u32> + '_ {
        self.block.state.resource_bindings()
    }
}

impl Device {
    pub fn prepare_resources(
        &self,
        descriptions: &[ResourceDesc],
    ) -> Result<PreparedResources, DeviceError> {
        self.available()?;
        if descriptions.is_empty() {
            return Err(DeviceError::InvalidCall);
        }
        let resources = self.resources.prepare_replace(&[], descriptions)?;
        let mut commands = Vec::new();
        commands
            .try_reserve_exact(resources.ids().len())
            .map_err(|_| DeviceError::Allocation)?;
        for (index, id) in resources.ids().iter().enumerate() {
            commands.push(Command::CreateResource {
                id: *id,
                desc: resources.desc(index),
                subresources: resources.infos(index),
            });
        }
        let commands = self.queue.prepare(&commands)?;
        Ok(PreparedResources {
            resources,
            commands,
        })
    }

    pub fn can_commit_resources(&self, prepared: &PreparedResources) -> Result<(), DeviceError> {
        self.available()?;
        self.resources.can_commit_replace(&prepared.resources)?;
        self.queue.can_commit(&prepared.commands)?;
        Ok(())
    }

    pub fn commit_resources(
        &mut self,
        prepared: PreparedResources,
    ) -> Result<Outcome<()>, DeviceError> {
        self.can_commit_resources(&prepared)?;
        let progress = self.queue.commit(prepared.commands)?;
        self.resources.commit_replace(prepared.resources)?;
        Ok(Outcome::new((), progress))
    }

    pub fn prepare_resource(
        &self,
        desc: ResourceDesc,
    ) -> Result<PreparedDeviceResource, DeviceError> {
        self.available()?;
        let resource = self.resources.prepare_create(desc)?;
        let commands = self.queue.prepare(&[Command::CreateResource {
            id: resource.id(),
            desc: resource.desc(),
            subresources: resource.infos(),
        }])?;
        Ok(PreparedDeviceResource { resource, commands })
    }

    pub fn can_commit_resource(
        &self,
        prepared: &PreparedDeviceResource,
    ) -> Result<(), DeviceError> {
        self.available()?;
        self.resources.can_commit_create(&prepared.resource)?;
        self.queue.can_commit(&prepared.commands)?;
        Ok(())
    }

    pub fn commit_resource(
        &mut self,
        prepared: PreparedDeviceResource,
    ) -> Result<Outcome<ResourceId>, DeviceError> {
        self.can_commit_resource(&prepared)?;
        let progress = self.queue.commit(prepared.commands)?;
        let id = self.resources.commit_create(prepared.resource)?;
        Ok(Outcome::new(id, progress))
    }

    pub fn prepare_vertex_shader(
        &mut self,
        declaration: &[u32],
        function: Option<&[u32]>,
        usage: u32,
    ) -> Result<PreparedDeviceShader, DeviceError> {
        self.available()?;
        let mode = self.description.behavior & 0xe0;
        if (mode == 0x20 && usage != 0x10) || (mode == 0x40 && usage != 0) {
            return Err(DeviceError::InvalidCall);
        }
        let shader = self.shaders.prepare_vertex(declaration, function, usage)?;
        let vertex = shader.vertex().unwrap();
        let commands = self.queue.prepare(&[Command::CreateShader {
            handle: shader.handle(),
            definition: ShaderDefinition::Vertex {
                declaration: &vertex.declaration,
                program: vertex.function.as_ref(),
            },
        }])?;
        Ok(PreparedDeviceShader { shader, commands })
    }

    pub fn prepare_pixel_shader(
        &mut self,
        function: &[u32],
    ) -> Result<PreparedDeviceShader, DeviceError> {
        self.available()?;
        let shader = self.shaders.prepare_pixel(function)?;
        let commands = self.queue.prepare(&[Command::CreateShader {
            handle: shader.handle(),
            definition: ShaderDefinition::Pixel {
                program: &shader.pixel().unwrap().function,
            },
        }])?;
        Ok(PreparedDeviceShader { shader, commands })
    }

    pub fn can_commit_shader(&self, prepared: &PreparedDeviceShader) -> Result<(), DeviceError> {
        self.available()?;
        self.shaders.can_commit(&prepared.shader)?;
        self.queue.can_commit(&prepared.commands)?;
        Ok(())
    }

    pub fn commit_shader(
        &mut self,
        prepared: PreparedDeviceShader,
    ) -> Result<Outcome<u32>, DeviceError> {
        self.can_commit_shader(&prepared)?;
        let progress = self.queue.commit(prepared.commands)?;
        let handle = self.shaders.commit(prepared.shader);
        Ok(Outcome::new(handle, progress))
    }

    pub fn prepare_state_block(
        &self,
        kind: StateBlockKind,
    ) -> Result<PreparedStateBlock, DeviceError> {
        self.available()?;
        let handle = self.prepared_block_handle()?;
        let state = self.state.create_state_block(kind)?;
        let vertex = state.vertex_shader().and_then(|_| self.vertex.clone());
        let fvf = state.vertex_shader().and_then(|_| self.fvf.clone());
        let pixel = state.pixel_shader().and_then(|_| self.pixel.clone());
        Ok(PreparedStateBlock {
            generation: self.generation,
            block: CapturedBlock {
                handle,
                state,
                vertex,
                fvf,
                pixel,
            },
            revision: self.state.revision(),
            recording_revision: self.state.recording_revision(),
            end_recording: false,
        })
    }

    pub fn prepare_end_state_block(&self) -> Result<PreparedStateBlock, DeviceError> {
        self.available()?;
        let handle = self.prepared_block_handle()?;
        let state = self.state.prepare_end_state_block()?;
        Ok(PreparedStateBlock {
            generation: self.generation,
            block: CapturedBlock {
                handle,
                state,
                vertex: self.recorded_vertex.clone().flatten(),
                fvf: self.recorded_fvf.clone().flatten(),
                pixel: self.recorded_pixel.clone().flatten(),
            },
            revision: self.state.revision(),
            recording_revision: self.state.recording_revision(),
            end_recording: true,
        })
    }

    pub fn can_commit_state_block(&self, prepared: &PreparedStateBlock) -> Result<(), DeviceError> {
        self.available()?;
        if self.generation != prepared.generation
            || self.prepared_block_handle()? != prepared.block.handle
            || self.state.revision() != prepared.revision
            || self.state.recording_revision() != prepared.recording_revision
            || self.state.is_recording() != prepared.end_recording
        {
            return Err(DeviceError::InvalidCall);
        }
        Ok(())
    }

    pub fn commit_state_block(&mut self, prepared: PreparedStateBlock) -> Result<u32, DeviceError> {
        self.can_commit_state_block(&prepared)?;
        if prepared.end_recording {
            self.state.end_state_block()?;
            self.recorded_vertex = None;
            self.recorded_fvf = None;
            self.recorded_pixel = None;
        }
        let handle = prepared.block.handle;
        self.blocks.push(prepared.block);
        self.block_sequence = handle;
        self.block_revision += 1;
        Ok(handle)
    }

    fn prepared_block_handle(&self) -> Result<u32, DeviceError> {
        if self.blocks.len() >= self.limits.state_blocks as usize {
            return Err(DeviceError::Capacity);
        }
        if self.block_revision == u64::MAX {
            return Err(DeviceError::IdentityExhausted);
        }
        self.block_sequence
            .checked_add(1)
            .ok_or(DeviceError::IdentityExhausted)
    }
}
