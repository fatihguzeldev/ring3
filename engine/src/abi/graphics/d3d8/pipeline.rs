use super::{CodecError, encoder::Encoder};
use crate::graphics::d3d8::state::{Light, MAX_LIGHTS, Matrix, PipelineState};

pub(super) fn encode(output: &mut Encoder<'_>, state: &PipelineState) -> Result<(), CodecError> {
    if state.lights.len() > MAX_LIGHTS {
        return Err(CodecError::InvalidCommand);
    }
    output.u32(2)?;
    output.count(state.render.len())?;
    output.words(&state.render)?;
    output.count(state.texture_stage.len())?;
    for stage in &state.texture_stage {
        output.count(stage.len())?;
        output.words(stage)?;
    }
    output.count(state.transforms.len())?;
    // pipeline v2 omits canonical identity matrices; little-endian mask bit i selects transform i.
    let mut selected = [0u32; 9];
    for (index, transform) in state.transforms.iter().enumerate() {
        if *transform != Matrix::IDENTITY {
            selected[index / 32] |= 1 << (index % 32);
        }
    }
    output.count(selected.len())?;
    output.words(&selected)?;
    for (index, transform) in state.transforms.iter().enumerate() {
        if selected[index / 32] & (1 << (index % 32)) != 0 {
            output.words(&transform.0)?;
        }
    }
    output.words(&state.material.diffuse)?;
    output.words(&state.material.ambient)?;
    output.words(&state.material.specular)?;
    output.words(&state.material.emissive)?;
    output.u32(state.material.power)?;
    output.count(state.lights.len())?;
    for entry in &state.lights {
        output.words(&[entry.index, u32::from(entry.enabled)])?;
        light(output, entry.light)?;
    }
    output.count(state.clip_planes.len())?;
    for plane in &state.clip_planes {
        output.words(plane)?;
    }
    output.words(&state.clip_status)?;
    let viewport = state.viewport;
    output.words(&[
        viewport.x,
        viewport.y,
        viewport.width,
        viewport.height,
        viewport.min_z,
        viewport.max_z,
        state.vertex_shader,
        state.pixel_shader,
        state.palette,
    ])?;
    output.count(state.vertex_constants.len())?;
    for constant in &state.vertex_constants {
        output.words(constant)?;
    }
    output.count(state.pixel_constants.len())?;
    for constant in &state.pixel_constants {
        output.words(constant)?;
    }
    Ok(())
}

fn light(output: &mut Encoder<'_>, light: Light) -> Result<(), CodecError> {
    output.u32(light.kind)?;
    output.words(&light.diffuse)?;
    output.words(&light.specular)?;
    output.words(&light.ambient)?;
    output.words(&light.position)?;
    output.words(&light.direction)?;
    output.words(&[light.range, light.falloff])?;
    output.words(&light.attenuation)?;
    output.words(&[light.theta, light.phi])
}
