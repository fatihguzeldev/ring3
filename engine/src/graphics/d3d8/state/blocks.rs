use super::{
    CLIP_PLANES, MAX_LIGHTS, PIXEL_CONSTANTS, PipelineState, StateError, TEXTURE_STAGES,
    VERTEX_CONSTANTS, VERTEX_STREAMS,
    render::{self, RENDER_COUNT, STAGE_COUNT, TRANSFORM_COUNT},
    types::LightState,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StateBlockKind {
    All,
    Pixel,
    Vertex,
}

pub struct StateBlock {
    pub(super) values: PipelineState,
    pub(super) mask: StateMask,
}

impl StateBlock {
    pub(crate) fn try_clone(&self) -> Result<Self, StateError> {
        Ok(Self {
            values: self.values.try_clone()?,
            mask: self.mask.clone(),
        })
    }
    pub(super) fn create(state: &PipelineState, kind: StateBlockKind) -> Result<Self, StateError> {
        let all = kind == StateBlockKind::All;
        let vertex = kind != StateBlockKind::Pixel;
        let pixel = kind != StateBlockKind::Vertex;
        let mut mask = StateMask {
            render: std::array::from_fn(|index| {
                (all && index != 165)
                    || (vertex && render::vertex_render(index))
                    || (pixel && render::pixel_render(index))
            }),
            texture_stage: std::array::from_fn(|_| {
                std::array::from_fn(|index| all || (vertex && render::vertex_stage(index)) || pixel)
            }),
            transforms: [all; TRANSFORM_COUNT],
            material: all,
            clip_planes: [all; CLIP_PLANES],
            clip_status: false,
            viewport: all,
            textures: [all; TEXTURE_STAGES],
            streams: [all; VERTEX_STREAMS],
            indices: all,
            vertex_shader: vertex,
            pixel_shader: pixel,
            vertex_constants: [vertex; VERTEX_CONSTANTS],
            pixel_constants: [pixel; PIXEL_CONSTANTS],
            palette: all,
            ..StateMask::default()
        };
        if vertex {
            for entry in &state.lights {
                mask.mark_light(entry.index, true, true);
            }
        }
        Ok(Self {
            values: state.try_clone()?,
            mask,
        })
    }

    pub fn resource_bindings(&self) -> impl Iterator<Item = u32> + '_ {
        self.values
            .textures
            .iter()
            .enumerate()
            .filter(|(index, _)| self.mask.textures[*index])
            .map(|(_, value)| *value)
            .chain(
                self.values
                    .streams
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| self.mask.streams[*index])
                    .map(|(_, value)| value.resource),
            )
            .chain(self.mask.indices.then_some(self.values.indices))
            .filter(|value| *value != 0)
    }

    pub fn vertex_shader(&self) -> Option<u32> {
        self.mask.vertex_shader.then_some(self.values.vertex_shader)
    }
    pub fn pixel_shader(&self) -> Option<u32> {
        self.mask.pixel_shader.then_some(self.values.pixel_shader)
    }

    pub(super) fn capture(&mut self, live: &PipelineState) -> Result<(), StateError> {
        self.values = live.try_clone()?;
        Ok(())
    }

    pub(super) fn apply(&self, target: &mut PipelineState) -> Result<(), StateError> {
        for (index, selected) in self.mask.render.iter().enumerate() {
            if *selected {
                target.render[index] = self.values.render[index];
            }
        }
        for stage in 0..TEXTURE_STAGES {
            for index in 0..STAGE_COUNT {
                if self.mask.texture_stage[stage][index] {
                    target.texture_stage[stage][index] = self.values.texture_stage[stage][index];
                }
            }
            if self.mask.textures[stage] {
                target.textures[stage] = self.values.textures[stage];
            }
        }
        for index in 0..TRANSFORM_COUNT {
            if self.mask.transforms[index] {
                target.transforms[index] = self.values.transforms[index];
            }
        }
        for index in 0..CLIP_PLANES {
            if self.mask.clip_planes[index] {
                target.clip_planes[index] = self.values.clip_planes[index];
            }
        }
        for index in 0..VERTEX_STREAMS {
            if self.mask.streams[index] {
                target.streams[index] = self.values.streams[index];
            }
        }
        for index in 0..VERTEX_CONSTANTS {
            if self.mask.vertex_constants[index] {
                target.vertex_constants[index] = self.values.vertex_constants[index];
            }
        }
        for index in 0..PIXEL_CONSTANTS {
            if self.mask.pixel_constants[index] {
                target.pixel_constants[index] = self.values.pixel_constants[index];
            }
        }
        if self.mask.material {
            target.material = self.values.material;
        }
        if self.mask.clip_status {
            target.clip_status = self.values.clip_status;
        }
        if self.mask.viewport {
            target.viewport = self.values.viewport;
        }
        if self.mask.indices {
            target.indices = self.values.indices;
            target.base_vertex = self.values.base_vertex;
        }
        if self.mask.vertex_shader {
            target.vertex_shader = self.values.vertex_shader;
        }
        if self.mask.pixel_shader {
            target.pixel_shader = self.values.pixel_shader;
        }
        if self.mask.palette {
            target.palette = self.values.palette;
        }
        for selected in self.mask.lights.iter().flatten() {
            let Some(source) = self
                .values
                .lights
                .iter()
                .find(|entry| entry.index == selected.index)
            else {
                continue;
            };
            let position = if let Some(position) = target
                .lights
                .iter()
                .position(|entry| entry.index == selected.index)
            {
                position
            } else {
                if target.lights.len() == MAX_LIGHTS {
                    return Err(StateError::LightCapacity);
                }
                target.lights.push(LightState {
                    index: selected.index,
                    light: Default::default(),
                    enabled: false,
                });
                target.lights.len() - 1
            };
            if selected.definition {
                target.lights[position].light = source.light;
            }
            if selected.enabled {
                target.lights[position].enabled = source.enabled;
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
pub(super) struct StateMask {
    pub render: [bool; RENDER_COUNT],
    pub texture_stage: [[bool; STAGE_COUNT]; TEXTURE_STAGES],
    pub transforms: [bool; TRANSFORM_COUNT],
    pub material: bool,
    pub lights: [Option<LightMask>; MAX_LIGHTS],
    pub clip_planes: [bool; CLIP_PLANES],
    pub clip_status: bool,
    pub viewport: bool,
    pub textures: [bool; TEXTURE_STAGES],
    pub streams: [bool; VERTEX_STREAMS],
    pub indices: bool,
    pub vertex_shader: bool,
    pub pixel_shader: bool,
    pub vertex_constants: [bool; VERTEX_CONSTANTS],
    pub pixel_constants: [bool; PIXEL_CONSTANTS],
    pub palette: bool,
}

impl Default for StateMask {
    fn default() -> Self {
        Self {
            render: [false; RENDER_COUNT],
            texture_stage: [[false; STAGE_COUNT]; TEXTURE_STAGES],
            transforms: [false; TRANSFORM_COUNT],
            material: false,
            lights: [None; MAX_LIGHTS],
            clip_planes: [false; CLIP_PLANES],
            clip_status: false,
            viewport: false,
            textures: [false; TEXTURE_STAGES],
            streams: [false; VERTEX_STREAMS],
            indices: false,
            vertex_shader: false,
            pixel_shader: false,
            vertex_constants: [false; VERTEX_CONSTANTS],
            pixel_constants: [false; PIXEL_CONSTANTS],
            palette: false,
        }
    }
}

impl StateMask {
    pub fn mark_light(&mut self, index: u32, definition: bool, enabled: bool) {
        let position = self
            .lights
            .iter()
            .position(|entry| entry.is_some_and(|entry| entry.index == index))
            .unwrap_or_else(|| self.lights.iter().position(Option::is_none).unwrap());
        let entry = self.lights[position].get_or_insert(LightMask {
            index,
            definition: false,
            enabled: false,
        });
        entry.definition |= definition;
        entry.enabled |= enabled;
    }
}

#[derive(Clone, Copy)]
pub(super) struct LightMask {
    index: u32,
    definition: bool,
    enabled: bool,
}
