use super::{
    CLIP_PLANES, Light, MAX_LIGHTS, Material, Matrix, PIXEL_CONSTANTS, StateBlock, StateBlockKind,
    StateError, StreamBinding, TEXTURE_STAGES, VERTEX_CONSTANTS, VERTEX_STREAMS, Viewport,
    blocks::StateMask,
    render::{self, RENDER_COUNT, STAGE_COUNT, TRANSFORM_COUNT},
    types::LightState,
};

#[derive(Debug, PartialEq, Eq)]
pub struct PipelineState {
    pub render: [u32; RENDER_COUNT],
    pub texture_stage: [[u32; STAGE_COUNT]; TEXTURE_STAGES],
    pub transforms: [Matrix; TRANSFORM_COUNT],
    pub material: Material,
    pub lights: Vec<LightState>,
    pub clip_planes: [[u32; 4]; CLIP_PLANES],
    pub clip_status: [u32; 2],
    pub viewport: Viewport,
    pub textures: [u32; TEXTURE_STAGES],
    pub streams: [StreamBinding; VERTEX_STREAMS],
    pub indices: u32,
    pub base_vertex: u32,
    pub vertex_shader: u32,
    pub pixel_shader: u32,
    pub vertex_constants: [[u32; 4]; VERTEX_CONSTANTS],
    pub pixel_constants: [[u32; 4]; PIXEL_CONSTANTS],
    pub palette: u32,
}

impl PipelineState {
    pub(super) fn try_clone(&self) -> Result<Self, StateError> {
        let mut lights = Vec::new();
        lights
            .try_reserve_exact(MAX_LIGHTS)
            .map_err(|_| StateError::Allocation)?;
        lights.extend_from_slice(&self.lights);
        Ok(Self {
            render: self.render,
            texture_stage: self.texture_stage,
            transforms: self.transforms,
            material: self.material,
            lights,
            clip_planes: self.clip_planes,
            clip_status: self.clip_status,
            viewport: self.viewport,
            textures: self.textures,
            streams: self.streams,
            indices: self.indices,
            base_vertex: self.base_vertex,
            vertex_shader: self.vertex_shader,
            pixel_shader: self.pixel_shader,
            vertex_constants: self.vertex_constants,
            pixel_constants: self.pixel_constants,
            palette: self.palette,
        })
    }

    pub fn resource_bindings(&self) -> impl Iterator<Item = u32> + '_ {
        self.textures
            .iter()
            .copied()
            .chain(self.streams.iter().map(|stream| stream.resource))
            .chain([self.indices])
            .filter(|resource| *resource != 0)
    }
}

pub struct DeviceState {
    pub(super) live: PipelineState,
    pub(super) recording: Option<StateBlock>,
    pub(super) revision: u64,
    recording_revision: u64,
    target_width: u32,
    target_height: u32,
}

impl DeviceState {
    pub(crate) fn try_clone(&self) -> Result<Self, StateError> {
        Ok(Self {
            live: self.live.try_clone()?,
            recording: self
                .recording
                .as_ref()
                .map(StateBlock::try_clone)
                .transpose()?,
            revision: self.revision,
            recording_revision: self.recording_revision,
            target_width: self.target_width,
            target_height: self.target_height,
        })
    }
    pub(crate) fn recorded_resources(&self) -> impl Iterator<Item = u32> + '_ {
        self.recording
            .iter()
            .flat_map(StateBlock::resource_bindings)
    }
    pub(crate) fn retained_binding(&self, kind: u8, index: usize) -> Option<u32> {
        let (values, selected) = if let Some(block) = &self.recording {
            (
                &block.values,
                match kind {
                    0 => block.mask.textures[index],
                    1 => block.mask.streams[index],
                    _ => block.mask.indices,
                },
            )
        } else {
            (&self.live, true)
        };
        let value = match kind {
            0 => values.textures[index],
            1 => values.streams[index].resource,
            _ => values.indices,
        };
        (selected && value != 0).then_some(value)
    }
    pub fn bind_vertex_shader(
        &mut self,
        handle: u32,
        constants: impl IntoIterator<Item = (u32, [u32; 4])>,
    ) -> Result<(), StateError> {
        let mut values = [None; VERTEX_CONSTANTS];
        for (index, value) in constants {
            *values
                .get_mut(index as usize)
                .ok_or(StateError::InvalidRange)? = Some(value);
        }
        let state = self.edit(|mask| {
            mask.vertex_shader = true;
            for (index, value) in values.iter().enumerate() {
                mask.vertex_constants[index] |= value.is_some();
            }
        })?;
        state.vertex_shader = handle;
        for (index, value) in values.into_iter().enumerate() {
            if let Some(value) = value {
                state.vertex_constants[index] = value;
            }
        }
        Ok(())
    }

    pub fn bind_pixel_shader(
        &mut self,
        handle: u32,
        constants: impl IntoIterator<Item = (u32, [u32; 4])>,
    ) -> Result<(), StateError> {
        let mut values = [None; PIXEL_CONSTANTS];
        for (index, value) in constants {
            *values
                .get_mut(index as usize)
                .ok_or(StateError::InvalidRange)? = Some(value);
        }
        let state = self.edit(|mask| {
            mask.pixel_shader = true;
            for (index, value) in values.iter().enumerate() {
                mask.pixel_constants[index] |= value.is_some();
            }
        })?;
        state.pixel_shader = handle;
        for (index, value) in values.into_iter().enumerate() {
            if let Some(value) = value {
                state.pixel_constants[index] = value;
            }
        }
        Ok(())
    }

    pub(crate) fn unbind_deleted_shader(
        &mut self,
        handle: u32,
        vertex: bool,
    ) -> Result<(), StateError> {
        let current = if vertex {
            &mut self.live.vertex_shader
        } else {
            &mut self.live.pixel_shader
        };
        if *current == handle {
            let revision = self
                .revision
                .checked_add(1)
                .ok_or(StateError::IdentityExhausted)?;
            *current = 0;
            self.revision = revision;
        }
        Ok(())
    }
    pub fn new(width: u32, height: u32, auto_depth: bool) -> Result<Self, StateError> {
        if width == 0 || height == 0 {
            return Err(StateError::InvalidRange);
        }
        let mut lights = Vec::new();
        lights
            .try_reserve_exact(MAX_LIGHTS)
            .map_err(|_| StateError::Allocation)?;
        Ok(Self {
            live: PipelineState {
                render: render::render_defaults(auto_depth),
                texture_stage: std::array::from_fn(render::stage_defaults),
                transforms: [Matrix::IDENTITY; TRANSFORM_COUNT],
                material: Material::default(),
                lights,
                clip_planes: [[0; 4]; CLIP_PLANES],
                clip_status: [0; 2],
                viewport: Viewport {
                    x: 0,
                    y: 0,
                    width,
                    height,
                    min_z: 0,
                    max_z: 0x3f80_0000,
                },
                textures: [0; TEXTURE_STAGES],
                streams: [StreamBinding::default(); VERTEX_STREAMS],
                indices: 0,
                base_vertex: 0,
                vertex_shader: 0,
                pixel_shader: 0,
                vertex_constants: [[0; 4]; VERTEX_CONSTANTS],
                pixel_constants: [[0; 4]; PIXEL_CONSTANTS],
                palette: 0,
            },
            recording: None,
            recording_revision: 0,
            revision: 1,
            target_width: width,
            target_height: height,
        })
    }

    pub fn snapshot(&self) -> &PipelineState {
        &self.live
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub(crate) fn recording_revision(&self) -> u64 {
        self.recording_revision
    }
    pub(crate) fn prepare_end_state_block(&self) -> Result<StateBlock, StateError> {
        self.recording
            .as_ref()
            .ok_or(StateError::NotRecording)?
            .try_clone()
    }
    pub(crate) fn finish_user_draw(&mut self, indexed: bool) -> Result<(), StateError> {
        if !self.user_draw_changes_bindings(indexed) {
            return Ok(());
        }
        let revision = self
            .revision
            .checked_add(1)
            .ok_or(StateError::IdentityExhausted)?;
        self.live.streams[0] = Default::default();
        if indexed {
            self.live.indices = 0;
            self.live.base_vertex = 0;
        }
        self.revision = revision;
        Ok(())
    }
    pub(crate) fn user_draw_changes_bindings(&self, indexed: bool) -> bool {
        self.live.streams[0] != StreamBinding::default()
            || (indexed && (self.live.indices != 0 || self.live.base_vertex != 0))
    }
    pub fn is_recording(&self) -> bool {
        self.recording.is_some()
    }

    fn pending(&self) -> &PipelineState {
        self.recording
            .as_ref()
            .map_or(&self.live, |block| &block.values)
    }

    fn edit(
        &mut self,
        mark: impl FnOnce(&mut StateMask),
    ) -> Result<&mut PipelineState, StateError> {
        if let Some(block) = &mut self.recording {
            self.recording_revision = self
                .recording_revision
                .checked_add(1)
                .ok_or(StateError::IdentityExhausted)?;
            mark(&mut block.mask);
            Ok(&mut block.values)
        } else {
            self.revision = self
                .revision
                .checked_add(1)
                .ok_or(StateError::IdentityExhausted)?;
            Ok(&mut self.live)
        }
    }

    pub fn render_state(&self, state: u32) -> Result<u32, StateError> {
        Ok(self.live.render[render::render_index(state)?])
    }

    pub fn set_render_state(&mut self, state: u32, value: u32) -> Result<(), StateError> {
        let index = render::render_index(state)?;
        if index == 165 {
            self.revision = self
                .revision
                .checked_add(1)
                .ok_or(StateError::IdentityExhausted)?;
            self.live.render[index] = value;
            return Ok(());
        }
        self.edit(|mask| mask.render[index] = true)?.render[index] = value;
        Ok(())
    }

    pub fn texture_stage_state(&self, stage: u32, state: u32) -> Result<u32, StateError> {
        let index = render::stage_index(state)?;
        Ok(self
            .live
            .texture_stage
            .get(stage as usize)
            .ok_or(StateError::InvalidRange)?[index])
    }

    pub fn set_texture_stage_state(
        &mut self,
        stage: u32,
        state: u32,
        value: u32,
    ) -> Result<(), StateError> {
        let index = render::stage_index(state)?;
        let stage = stage as usize;
        if stage >= TEXTURE_STAGES {
            return Err(StateError::InvalidRange);
        }
        self.edit(|mask| mask.texture_stage[stage][index] = true)?
            .texture_stage[stage][index] = value;
        Ok(())
    }

    pub fn transform(&self, state: u32) -> Result<Matrix, StateError> {
        Ok(self.live.transforms[render::transform_index(state)?])
    }

    pub fn set_transform(&mut self, state: u32, matrix: Matrix) -> Result<(), StateError> {
        let index = render::transform_index(state)?;
        self.edit(|mask| mask.transforms[index] = true)?.transforms[index] = matrix;
        Ok(())
    }

    pub fn multiply_transform(&mut self, state: u32, matrix: Matrix) -> Result<(), StateError> {
        let index = render::transform_index(state)?;
        let result = matrix.multiply(self.pending().transforms[index]);
        self.set_transform(state, result)
    }

    pub fn set_material(&mut self, material: Material) -> Result<(), StateError> {
        self.edit(|mask| mask.material = true)?.material = material;
        Ok(())
    }

    pub fn light(&self, index: u32) -> Result<Light, StateError> {
        self.live
            .lights
            .iter()
            .find(|entry| entry.index == index)
            .map(|entry| entry.light)
            .ok_or(StateError::InvalidLight)
    }

    pub fn light_enabled(&self, index: u32) -> Result<bool, StateError> {
        self.live
            .lights
            .iter()
            .find(|entry| entry.index == index)
            .map(|entry| entry.enabled)
            .ok_or(StateError::InvalidLight)
    }

    pub fn set_light(&mut self, index: u32, light: Light) -> Result<(), StateError> {
        light.validate()?;
        let existing = self
            .pending()
            .lights
            .iter()
            .position(|entry| entry.index == index);
        if existing.is_none() && self.pending().lights.len() == MAX_LIGHTS {
            return Err(StateError::LightCapacity);
        }
        let state = self.edit(|mask| mask.mark_light(index, true, false))?;
        if let Some(position) = existing {
            state.lights[position].light = light;
        } else {
            state.lights.push(LightState {
                index,
                light,
                enabled: false,
            });
        }
        Ok(())
    }

    pub fn enable_light(&mut self, index: u32, enabled: bool) -> Result<(), StateError> {
        let existing = self
            .pending()
            .lights
            .iter()
            .position(|entry| entry.index == index);
        if existing.is_none() && self.pending().lights.len() == MAX_LIGHTS {
            return Err(StateError::LightCapacity);
        }
        let state = self.edit(|mask| mask.mark_light(index, existing.is_none(), true))?;
        if let Some(position) = existing {
            state.lights[position].enabled = enabled;
        } else {
            state.lights.push(LightState {
                index,
                light: Light::default(),
                enabled,
            });
        }
        Ok(())
    }

    pub fn set_clip_plane(&mut self, index: u32, plane: [u32; 4]) -> Result<(), StateError> {
        let index = index as usize;
        if index >= CLIP_PLANES {
            return Err(StateError::InvalidRange);
        }
        self.edit(|mask| mask.clip_planes[index] = true)?
            .clip_planes[index] = plane;
        Ok(())
    }

    pub fn set_clip_status(&mut self, status: [u32; 2]) -> Result<(), StateError> {
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or(StateError::IdentityExhausted)?;
        self.live.clip_status = status;
        Ok(())
    }

    pub fn set_viewport(&mut self, viewport: Viewport) -> Result<(), StateError> {
        viewport.validate(self.target_width, self.target_height)?;
        self.edit(|mask| mask.viewport = true)?.viewport = viewport;
        Ok(())
    }

    pub fn set_target_size(&mut self, width: u32, height: u32) -> Result<(), StateError> {
        if width == 0 || height == 0 {
            return Err(StateError::InvalidRange);
        }
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or(StateError::IdentityExhausted)?;
        self.target_width = width;
        self.target_height = height;
        self.live.viewport = Viewport {
            x: 0,
            y: 0,
            width,
            height,
            min_z: 0,
            max_z: 0x3f80_0000,
        };
        Ok(())
    }

    pub fn set_texture(&mut self, stage: u32, resource: u32) -> Result<(), StateError> {
        let stage = stage as usize;
        if stage >= TEXTURE_STAGES {
            return Err(StateError::InvalidRange);
        }
        self.edit(|mask| mask.textures[stage] = true)?.textures[stage] = resource;
        Ok(())
    }

    pub fn set_stream(
        &mut self,
        stream: u32,
        mut binding: StreamBinding,
    ) -> Result<(), StateError> {
        let stream = stream as usize;
        if stream >= VERTEX_STREAMS {
            return Err(StateError::InvalidRange);
        }
        if binding.resource == 0 {
            binding.stride = self.pending().streams[stream].stride;
        }
        self.edit(|mask| mask.streams[stream] = true)?.streams[stream] = binding;
        Ok(())
    }

    pub fn set_indices(&mut self, resource: u32, base_vertex: u32) -> Result<(), StateError> {
        let state = self.edit(|mask| mask.indices = true)?;
        state.indices = resource;
        state.base_vertex = base_vertex;
        Ok(())
    }

    pub fn set_vertex_shader(&mut self, handle: u32) -> Result<(), StateError> {
        self.edit(|mask| mask.vertex_shader = true)?.vertex_shader = handle;
        Ok(())
    }

    pub fn set_pixel_shader(&mut self, handle: u32) -> Result<(), StateError> {
        self.edit(|mask| mask.pixel_shader = true)?.pixel_shader = handle;
        Ok(())
    }

    pub fn set_vertex_constants(
        &mut self,
        start: u32,
        values: &[[u32; 4]],
    ) -> Result<(), StateError> {
        let range = constant_range(start, values.len(), VERTEX_CONSTANTS)?;
        self.edit(|mask| mask.vertex_constants[range.clone()].fill(true))?
            .vertex_constants[range]
            .copy_from_slice(values);
        Ok(())
    }

    pub fn set_pixel_constants(
        &mut self,
        start: u32,
        values: &[[u32; 4]],
    ) -> Result<(), StateError> {
        let range = constant_range(start, values.len(), PIXEL_CONSTANTS)?;
        self.edit(|mask| mask.pixel_constants[range.clone()].fill(true))?
            .pixel_constants[range]
            .copy_from_slice(values);
        Ok(())
    }

    pub fn set_palette(&mut self, palette: u32) -> Result<(), StateError> {
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or(StateError::IdentityExhausted)?;
        self.live.palette = palette;
        Ok(())
    }

    pub fn begin_state_block(&mut self) -> Result<(), StateError> {
        if self.is_recording() {
            return Err(StateError::Recording);
        }
        let revision = self
            .recording_revision
            .checked_add(1)
            .ok_or(StateError::IdentityExhausted)?;
        self.recording = Some(StateBlock {
            values: self.live.try_clone()?,
            mask: StateMask::default(),
        });
        self.recording_revision = revision;
        Ok(())
    }

    pub fn end_state_block(&mut self) -> Result<StateBlock, StateError> {
        self.recording.take().ok_or(StateError::NotRecording)
    }

    pub fn create_state_block(&self, kind: StateBlockKind) -> Result<StateBlock, StateError> {
        if self.is_recording() {
            return Err(StateError::Recording);
        }
        StateBlock::create(&self.live, kind)
    }

    pub fn capture_state_block(&self, block: &mut StateBlock) -> Result<(), StateError> {
        if self.is_recording() {
            return Err(StateError::Recording);
        }
        block.capture(&self.live)
    }

    pub fn apply_state_block(&mut self, block: &StateBlock) -> Result<(), StateError> {
        if self.is_recording() {
            return Err(StateError::Recording);
        }
        let revision = self
            .revision
            .checked_add(1)
            .ok_or(StateError::IdentityExhausted)?;
        let mut candidate = self.live.try_clone()?;
        block.apply(&mut candidate)?;
        candidate
            .viewport
            .validate(self.target_width, self.target_height)?;
        self.live = candidate;
        self.revision = revision;
        Ok(())
    }
}

fn constant_range(
    start: u32,
    count: usize,
    limit: usize,
) -> Result<std::ops::Range<usize>, StateError> {
    let start = start as usize;
    let end = start
        .checked_add(count)
        .filter(|end| *end <= limit)
        .ok_or(StateError::InvalidRange)?;
    Ok(start..end)
}
