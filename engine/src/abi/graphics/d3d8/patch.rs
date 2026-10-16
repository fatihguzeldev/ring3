use super::{CodecError, encoder::Encoder};
use crate::graphics::d3d8::{
    commands::{PatchDraw, PatchSource, StreamResource},
    patch::{
        domain::DomainMesh,
        program::{InputPlan, SurfaceProgram},
    },
    state::VERTEX_STREAMS,
};

pub(super) fn geometry(
    output: &mut Encoder<'_>,
    program: &SurfaceProgram,
    inputs: &InputPlan,
    streams: &[Option<StreamResource>; VERTEX_STREAMS],
) -> Result<(), CodecError> {
    if streams.iter().all(Option::is_none) {
        return Err(CodecError::InvalidCommand);
    }
    output.words(program.words())?;
    output.words(inputs.words())?;
    for stream in streams {
        output.u32(u32::from(stream.is_some()))?;
        if let Some(stream) = stream {
            if stream.stride > 2048 {
                return Err(CodecError::InvalidCommand);
            }
            output.version(stream.resource)?;
            output.u32(stream.stride)?;
        }
    }
    Ok(())
}

pub(super) fn domain(
    output: &mut Encoder<'_>,
    identity: u64,
    mesh: &DomainMesh,
) -> Result<(), CodecError> {
    if identity == 0 {
        return Err(CodecError::Identity);
    }
    let key = mesh.key().ok_or(CodecError::InvalidCommand)?;
    output.u64(identity)?;
    output.words(&[
        u32::from(key.triangular),
        u32::from(key.style == crate::graphics::d3d8::patch::domain::EdgeStyle::Continuous),
    ])?;
    output.words(&key.segments.words())?;
    output.count(mesh.vertices().len())?;
    output.count(mesh.indices().len())?;
    for vertex in mesh.vertices() {
        output.words(&vertex.map(f32::to_bits))?;
    }
    output.words(mesh.indices())?;
    Ok(())
}

pub(super) fn draw(output: &mut Encoder<'_>, draw: &PatchDraw<'_>) -> Result<(), CodecError> {
    if draw.pipeline.0 == 0
        || draw.domain == 0
        || draw.bindings.streams.iter().any(Option::is_some)
        || draw.bindings.indices.is_some()
    {
        return Err(CodecError::InvalidCommand);
    }
    output.u64(draw.pipeline.0)?;
    output.u64(draw.domain)?;
    output.targets(draw.targets)?;
    super::command::draw_bindings(output, &draw.bindings)?;
    match draw.source {
        PatchSource::Cached { identity } => {
            if identity == 0 {
                return Err(CodecError::Identity);
            }
            output.u32(0)?;
            output.u64(identity)?;
        }
        PatchSource::Inline {
            program,
            inputs,
            streams,
        } => {
            output.u32(1)?;
            geometry(output, program, inputs, streams)?;
        }
    }
    Ok(())
}
