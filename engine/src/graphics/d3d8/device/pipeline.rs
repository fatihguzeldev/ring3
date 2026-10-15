use super::super::{
    commands::{Command, Progress},
    shader::{
        Fvf,
        registry::{RegistryError, is_shader_handle},
    },
    state::StateBlockKind,
};
use super::context::RetiredShader;
use super::{Device, DeviceError, Outcome};
use std::sync::Arc;

impl Device {
    pub fn create_vertex_shader(
        &mut self,
        declaration: &[u32],
        function: Option<&[u32]>,
        usage: u32,
    ) -> Result<Outcome<u32>, DeviceError> {
        self.drain_retired()?;
        let prepared = self.prepare_vertex_shader(declaration, function, usage)?;
        self.commit_shader(prepared)
    }

    pub fn create_pixel_shader(&mut self, function: &[u32]) -> Result<Outcome<u32>, DeviceError> {
        self.drain_retired()?;
        let prepared = self.prepare_pixel_shader(function)?;
        self.commit_shader(prepared)
    }

    pub fn bind_vertex_shader(&mut self, handle: u32) -> Result<(), DeviceError> {
        self.available()?;
        let shader = if is_shader_handle(handle) {
            Some(self.shaders.acquire_vertex(handle)?)
        } else {
            None
        };
        let fvf = if shader.is_none() && handle != 0 {
            Some(Arc::new(Fvf::parse(handle).map_err(RegistryError::from)?))
        } else {
            None
        };
        self.state.bind_vertex_shader(
            handle,
            shader.iter().flat_map(|shader| shader.bind_constants()),
        )?;
        if self.state.is_recording() {
            self.recorded_vertex = Some(shader);
            self.recorded_fvf = Some(fvf);
        } else {
            self.vertex = shader;
            self.fvf = fvf;
        }
        Ok(())
    }

    pub fn bind_pixel_shader(&mut self, handle: u32) -> Result<(), DeviceError> {
        self.available()?;
        let shader = if handle == 0 {
            None
        } else {
            Some(self.shaders.acquire_pixel(handle)?)
        };
        self.state.bind_pixel_shader(
            handle,
            shader.iter().flat_map(|shader| shader.bind_constants()),
        )?;
        if self.state.is_recording() {
            self.recorded_pixel = Some(shader);
        } else {
            self.pixel = shader;
        }
        Ok(())
    }

    pub fn delete_vertex_shader(&mut self, handle: u32) -> Result<(), DeviceError> {
        self.shaders.vertex(handle)?;
        self.state.unbind_deleted_shader(handle, true)?;
        if self.state.snapshot().vertex_shader == 0 {
            self.vertex = None;
            self.fvf = None;
        }
        let shader = self.shaders.delete_vertex(handle)?;
        self.retired_shaders
            .push(RetiredShader::Vertex(handle, shader));
        Ok(())
    }

    pub fn delete_pixel_shader(&mut self, handle: u32) -> Result<(), DeviceError> {
        self.shaders.pixel(handle)?;
        self.state.unbind_deleted_shader(handle, false)?;
        if self.state.snapshot().pixel_shader == 0 {
            self.pixel = None;
        }
        let shader = self.shaders.delete_pixel(handle)?;
        self.retired_shaders
            .push(RetiredShader::Pixel(handle, shader));
        Ok(())
    }

    pub fn flush_retired_shaders(&mut self) -> Result<Outcome<()>, DeviceError> {
        if !self.retired_shaders.iter().any(RetiredShader::ready) {
            return Ok(Outcome::new((), Progress::Queued));
        }
        let mut commands = Vec::new();
        commands
            .try_reserve_exact(self.retired_shaders.len())
            .map_err(|_| DeviceError::Allocation)?;
        for shader in self.retired_shaders.iter().filter(|shader| shader.ready()) {
            commands.push(Command::DeleteShader {
                handle: shader.handle(),
            });
        }
        let prepared = self.queue.prepare(&commands)?;
        let progress = self.queue.commit(prepared)?;
        self.retired_shaders.retain(|shader| !shader.ready());
        self.shaders.retained_bytes();
        Ok(Outcome::new((), progress))
    }

    pub(super) fn drain_retired(&mut self) -> Result<(), DeviceError> {
        if self.flush_retired_shaders()?.progress == Progress::Draining {
            return Err(super::super::commands::CommandError::Backpressure.into());
        }
        Ok(())
    }

    pub fn begin_state_block(&mut self) -> Result<(), DeviceError> {
        self.available()?;
        self.state.begin_state_block()?;
        self.recorded_vertex = None;
        self.recorded_fvf = None;
        self.recorded_pixel = None;
        Ok(())
    }

    pub fn end_state_block(&mut self) -> Result<u32, DeviceError> {
        let prepared = self.prepare_end_state_block()?;
        self.commit_state_block(prepared)
    }

    pub fn create_state_block(&mut self, kind: StateBlockKind) -> Result<u32, DeviceError> {
        let prepared = self.prepare_state_block(kind)?;
        self.commit_state_block(prepared)
    }

    pub fn capture_state_block(&mut self, handle: u32) -> Result<(), DeviceError> {
        let prepared = self.prepare_capture_state_block(handle)?;
        self.commit_block_change(prepared)
    }

    pub fn apply_state_block(&mut self, handle: u32) -> Result<(), DeviceError> {
        let prepared = self.prepare_apply_state_block(handle)?;
        self.commit_block_change(prepared)
    }

    pub fn delete_state_block(&mut self, handle: u32) -> Result<(), DeviceError> {
        let prepared = self.prepare_delete_state_block(handle)?;
        self.commit_block_change(prepared)
    }

    pub fn state_block_resources(
        &self,
        handle: u32,
    ) -> Result<impl Iterator<Item = u32> + '_, DeviceError> {
        Ok(self.blocks[self.block_index(handle)?]
            .state
            .resource_bindings())
    }

    pub(super) fn block_index(&self, handle: u32) -> Result<usize, DeviceError> {
        self.blocks
            .binary_search_by_key(&handle, |block| block.handle)
            .map_err(|_| DeviceError::InvalidCall)
    }
}
