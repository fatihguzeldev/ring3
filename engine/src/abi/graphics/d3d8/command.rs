use super::{
    CodecError, FIRST_FRAGMENT, FRAGMENT_HEADER_SIZE, LAST_FRAGMENT, encoder::Encoder, pipeline,
    shader,
};
use crate::graphics::d3d8::{
    commands::{Command, Draw, DrawData},
    resources::{BufferKind, Format, ResourceDesc},
};

pub fn payload_size(command: &Command<'_>) -> Result<(u16, usize), CodecError> {
    let mut output = Encoder::new(None);
    let opcode = encode(&mut output, command)?;
    Ok((opcode, output.length))
}

pub fn append_payload(command: &Command<'_>, output: &mut Vec<u8>) -> Result<(), CodecError> {
    let (_, length) = payload_size(command)?;
    output
        .try_reserve(length)
        .map_err(|_| CodecError::Allocation)?;
    let start = output.len();
    encode(&mut Encoder::new(Some(output)), command)?;
    debug_assert_eq!(output.len() - start, length);
    Ok(())
}

pub fn append_command(
    command: &Command<'_>,
    identity: u64,
    output: &mut Vec<u8>,
) -> Result<(), CodecError> {
    let (opcode, length) = payload_size(command)?;
    let record_length = length
        .checked_add(FRAGMENT_HEADER_SIZE)
        .and_then(|length| u32::try_from(length).ok())
        .ok_or(CodecError::Length)?;
    if identity == 0 {
        return Err(CodecError::Identity);
    }
    output
        .try_reserve(record_length as usize)
        .map_err(|_| CodecError::Allocation)?;
    output.extend_from_slice(&opcode.to_le_bytes());
    output.extend_from_slice(&(FIRST_FRAGMENT | LAST_FRAGMENT).to_le_bytes());
    output.extend_from_slice(&record_length.to_le_bytes());
    output.extend_from_slice(&identity.to_le_bytes());
    output.extend_from_slice(&0u32.to_le_bytes());
    output.extend_from_slice(&(length as u32).to_le_bytes());
    output.extend_from_slice(&[0; 8]);
    encode(&mut Encoder::new(Some(output)), command)?;
    Ok(())
}

fn encode(output: &mut Encoder<'_>, command: &Command<'_>) -> Result<u16, CodecError> {
    Ok(match command {
        Command::CreateResource {
            id,
            desc,
            subresources,
        } => {
            output.resource(*id)?;
            let (width, height, depth, levels, samples, lockable, fvf) = match *desc {
                ResourceDesc::Buffer(desc) => (
                    desc.length,
                    1,
                    1,
                    1,
                    0,
                    1,
                    match desc.kind {
                        BufferKind::Vertex { fvf } => fvf,
                        _ => 0,
                    },
                ),
                ResourceDesc::Texture(desc) => {
                    (desc.width, desc.height, desc.depth, desc.levels, 0, 0, 0)
                }
                ResourceDesc::Surface(desc) => (
                    desc.width,
                    desc.height,
                    1,
                    1,
                    desc.multisample,
                    u32::from(desc.lockable),
                    0,
                ),
            };
            if subresources.is_empty() || width == 0 || height == 0 || depth == 0 || levels == 0 {
                return Err(CodecError::InvalidCommand);
            }
            output.words(&[
                desc.resource_type(),
                desc.format() as u32,
                desc.usage().0,
                desc.pool() as u32,
                width,
                height,
                depth,
                levels,
                samples,
                lockable,
                fvf,
                0,
            ])?;
            output.count(subresources.len())?;
            for info in *subresources {
                output.words(&[
                    info.width,
                    info.height,
                    info.depth,
                    info.row_pitch,
                    info.slice_pitch,
                    info.length,
                    info.face,
                    info.level,
                ])?;
            }
            1
        }
        Command::DestroyResource(resource) => {
            output.version(*resource)?;
            2
        }
        Command::Upload(upload) => {
            if upload.row_bytes == 0
                || upload.rows == 0
                || upload.slices == 0
                || upload.row_pitch < upload.row_bytes
            {
                return Err(CodecError::InvalidCommand);
            }
            let slice_length = (upload.rows - 1)
                .checked_mul(upload.row_pitch)
                .and_then(|offset| offset.checked_add(upload.row_bytes))
                .ok_or(CodecError::Length)?;
            if upload.slice_pitch < slice_length {
                return Err(CodecError::InvalidCommand);
            }
            let length = (upload.slices - 1)
                .checked_mul(upload.slice_pitch)
                .and_then(|offset| offset.checked_add(slice_length))
                .ok_or(CodecError::Length)?;
            if upload.bytes.len() != length as usize || upload.offset.checked_add(length).is_none()
            {
                return Err(CodecError::Length);
            }
            output.version(upload.resource)?;
            output.words(&[
                upload.subresource,
                upload.offset,
                upload.row_pitch,
                upload.slice_pitch,
                upload.row_bytes,
                upload.rows,
                upload.slices,
                length,
            ])?;
            output.bytes(upload.bytes)?;
            3
        }
        Command::CreateShader { handle, definition } => {
            if *handle == 0 {
                return Err(CodecError::Identity);
            }
            output.u32(*handle)?;
            shader::encode(output, definition)?;
            4
        }
        Command::DeleteShader { handle } => {
            if *handle == 0 {
                return Err(CodecError::Identity);
            }
            output.u32(*handle)?;
            5
        }
        Command::Pipeline { id, state } => {
            if id.0 == 0 {
                return Err(CodecError::Identity);
            }
            output.u64(id.0)?;
            pipeline::encode(output, state)?;
            6
        }
        Command::Targets(targets) => {
            output.targets(*targets)?;
            7
        }
        Command::Draw(command) => {
            draw(output, command)?;
            8
        }
        Command::Clear {
            targets,
            flags,
            color,
            depth,
            stencil,
            rectangles,
        } => {
            if *flags == 0 || flags & !7 != 0 {
                return Err(CodecError::InvalidCommand);
            }
            if flags & 2 != 0 {
                let depth = f32::from_bits(*depth);
                if !depth.is_finite() || !(0.0..=1.0).contains(&depth) {
                    return Err(CodecError::InvalidCommand);
                }
            }
            output.targets(*targets)?;
            output.words(&[*flags, *color, *depth, *stencil])?;
            output.count(rectangles.len())?;
            for rectangle in *rectangles {
                output.rect(*rectangle)?;
            }
            9
        }
        Command::Copy {
            source,
            destination,
            rectangles,
        } => {
            if source.surface == destination.surface {
                return Err(CodecError::InvalidCommand);
            }
            output.surface(*source)?;
            output.surface(*destination)?;
            output.count(rectangles.len())?;
            for rectangle in *rectangles {
                output.rect(rectangle.source)?;
                output.words(&[rectangle.destination.0, rectangle.destination.1])?;
            }
            10
        }
        Command::Present(present) => {
            if present.swap_chain == 0
                || present.window == 0
                || !(1..=4).contains(&present.swap_effect)
                || !present.dirty_region && !present.dirty_rects.is_empty()
            {
                return Err(CodecError::Identity);
            }
            output.u64(present.swap_chain)?;
            output.words(&[
                present.window,
                present.interval,
                present.swap_effect,
                u32::from(present.dirty_region),
            ])?;
            output.surface(present.source)?;
            output.u32(u32::from(present.source_rect.is_some()))?;
            if let Some(rectangle) = present.source_rect {
                output.rect(rectangle)?;
            }
            output.u32(u32::from(present.destination_rect.is_some()))?;
            if let Some(rectangle) = present.destination_rect {
                if rectangle.left >= rectangle.right || rectangle.top >= rectangle.bottom {
                    return Err(CodecError::InvalidCommand);
                }
                output.words(&[
                    rectangle.left as u32,
                    rectangle.top as u32,
                    rectangle.right as u32,
                    rectangle.bottom as u32,
                ])?;
            }
            output.count(present.dirty_rects.len())?;
            for rectangle in present.dirty_rects {
                output.rect(*rectangle)?;
            }
            11
        }
        Command::Reset(reset) => {
            if reset.generation == 0
                || reset.width == 0
                || reset.height == 0
                || reset.back_buffers.is_empty()
                || reset.back_buffers.len() > 3
            {
                return Err(CodecError::InvalidCommand);
            }
            output.u64(reset.generation)?;
            output.words(&[
                reset.width,
                reset.height,
                reset.format as u32,
                reset.multisample,
            ])?;
            output.count(reset.back_buffers.len())?;
            for surface in reset.back_buffers {
                output.surface(*surface)?;
            }
            output.u32(u32::from(reset.depth.is_some()))?;
            if let Some(depth) = reset.depth {
                output.surface(depth)?;
            }
            12
        }
        Command::DeviceEvent { generation, event } => {
            if *generation == 0 {
                return Err(CodecError::Identity);
            }
            output.u64(*generation)?;
            output.u32(*event as u32)?;
            13
        }
        Command::Readback(request) => {
            if request.id.epoch == 0 || request.id.sequence == 0 {
                return Err(CodecError::Identity);
            }
            output.u64(request.id.epoch)?;
            output.u64(request.id.sequence)?;
            output.surface(request.source)?;
            output.u32(request.kind as u32)?;
            let layout = request.expected;
            let (block_width, block_height, block_bytes) = layout.format.block();
            let row_bytes = if layout.format.is_buffer() {
                Some(layout.width)
            } else {
                layout.width.div_ceil(block_width).checked_mul(block_bytes)
            };
            if layout.width == 0
                || layout.height == 0
                || layout.depth == 0
                || row_bytes.is_none_or(|bytes| bytes > layout.row_pitch)
                || layout
                    .row_pitch
                    .checked_mul(layout.height.div_ceil(block_height))
                    .is_none_or(|bytes| bytes > layout.slice_pitch)
                || layout.slice_pitch.checked_mul(layout.depth) != Some(layout.length)
            {
                return Err(CodecError::InvalidCommand);
            }
            output.words(&[
                1,
                layout.format as u32,
                layout.width,
                layout.height,
                layout.depth,
                layout.row_pitch,
                layout.slice_pitch,
                layout.length,
            ])?;
            14
        }
        Command::Palette { index, entries } => {
            output.u32(*index)?;
            for entry in *entries {
                output.bytes(entry)?;
            }
            15
        }
        Command::Gamma { flags, ramp } => {
            output.u32(*flags)?;
            for channel in *ramp {
                for value in channel {
                    output.bytes(&value.to_le_bytes())?;
                }
            }
            16
        }
        Command::CursorShape { source, hotspot } => {
            output.surface(*source)?;
            output.words(&[hotspot.0, hotspot.1])?;
            17
        }
        Command::CursorPosition { x, y, flags } => {
            output.words(&[*x as u32, *y as u32, *flags])?;
            18
        }
        Command::CursorVisible(visible) => {
            output.u32(u32::from(*visible))?;
            19
        }
        Command::ProcessVertices {
            pipeline,
            bindings,
            source_start,
            destination,
            destination_index,
            vertex_count,
            flags,
            destination_fvf,
        } => {
            if pipeline.0 == 0 {
                return Err(CodecError::Identity);
            }
            output.u64(pipeline.0)?;
            draw_bindings(output, bindings)?;
            output.version(*destination)?;
            output.words(&[
                *source_start,
                *destination_index,
                *vertex_count,
                *flags,
                *destination_fvf,
            ])?;
            20
        }
    })
}

fn draw(output: &mut Encoder<'_>, draw: &Draw<'_>) -> Result<(), CodecError> {
    if draw.pipeline.0 == 0 {
        return Err(CodecError::Identity);
    }
    let elements = draw
        .primitive
        .element_count(draw.primitive_count)
        .ok_or(CodecError::Length)?;
    output.u64(draw.pipeline.0)?;
    output.words(&[draw.primitive as u32, draw.primitive_count])?;
    output.targets(draw.targets)?;
    draw_bindings(output, &draw.bindings)?;
    match &draw.data {
        DrawData::Vertices { first_vertex } => output.words(&[0, *first_vertex])?,
        DrawData::Indexed {
            min_vertex,
            vertex_count,
            start_index,
        } => {
            if draw.bindings.indices.is_none() || min_vertex.checked_add(*vertex_count).is_none() {
                return Err(CodecError::InvalidCommand);
            }
            output.words(&[1, *min_vertex, *vertex_count, *start_index])?;
        }
        DrawData::UserVertices { stride, vertices } => {
            if *stride == 0
                || elements
                    .checked_mul(*stride)
                    .is_none_or(|length| length as usize != vertices.len())
            {
                return Err(CodecError::Length);
            }
            output.words(&[2, *stride])?;
            output.count(vertices.len())?;
            output.bytes(vertices)?;
        }
        DrawData::UserIndexed {
            min_vertex,
            vertex_count,
            stride,
            vertices,
            format,
            indices,
        } => {
            let index_size = index_size(*format)?;
            let maximum = min_vertex
                .checked_add(*vertex_count)
                .ok_or(CodecError::Length)?;
            if *stride == 0
                || vertex_count
                    .checked_mul(*stride)
                    .is_none_or(|length| length as usize != vertices.len())
                || elements
                    .checked_mul(index_size)
                    .is_none_or(|length| length as usize != indices.len())
            {
                return Err(CodecError::Length);
            }
            for bytes in indices.chunks_exact(index_size as usize) {
                let index = if index_size == 2 {
                    u32::from(u16::from_le_bytes(bytes.try_into().unwrap()))
                } else {
                    u32::from_le_bytes(bytes.try_into().unwrap())
                };
                if index < *min_vertex || index >= maximum {
                    return Err(CodecError::InvalidCommand);
                }
            }
            output.words(&[3, *min_vertex, *vertex_count, *stride, *format as u32])?;
            output.count(vertices.len())?;
            output.count(indices.len())?;
            output.bytes(vertices)?;
            output.bytes(indices)?;
        }
    }
    Ok(())
}

fn draw_bindings(
    output: &mut Encoder<'_>,
    bindings: &crate::graphics::d3d8::commands::DrawBindings,
) -> Result<(), CodecError> {
    for texture in bindings.textures {
        output.u32(u32::from(texture.is_some()))?;
        if let Some(texture) = texture {
            output.version(texture.resource)?;
            output.u32(texture.lod)?;
        }
    }
    for stream in bindings.streams {
        output.u32(u32::from(stream.is_some()))?;
        if let Some(stream) = stream {
            if stream.stride == 0 {
                return Err(CodecError::InvalidCommand);
            }
            output.version(stream.resource)?;
            output.u32(stream.stride)?;
        }
    }
    output.u32(u32::from(bindings.indices.is_some()))?;
    if let Some(indices) = bindings.indices {
        index_size(indices.format)?;
        output.version(indices.resource)?;
        output.words(&[indices.format as u32, indices.base_vertex])?;
    }
    Ok(())
}

fn index_size(format: Format) -> Result<u32, CodecError> {
    match format {
        Format::Index16 => Ok(2),
        Format::Index32 => Ok(4),
        _ => Err(CodecError::InvalidCommand),
    }
}
