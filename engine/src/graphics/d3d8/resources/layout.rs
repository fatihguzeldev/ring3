use super::{
    BufferKind, Format, Pool, ResourceDesc, ResourceError, SubresourceInfo, TextureKind, Usage,
};

pub(super) fn describe(
    mut desc: ResourceDesc,
) -> Result<(ResourceDesc, Vec<SubresourceInfo>, usize), ResourceError> {
    let usage = desc.usage();
    if usage.0 & !Usage::KNOWN != 0
        || (usage.contains(Usage::DYNAMIC) && matches!(desc.pool(), Pool::Managed | Pool::Scratch))
    {
        return Err(ResourceError::InvalidCall);
    }
    let mut layouts = Vec::new();
    let mut total = 0usize;
    match &mut desc {
        ResourceDesc::Buffer(buffer) => {
            if buffer.length == 0
                || buffer.pool == Pool::Scratch
                || usage.0 & (Usage::RENDER_TARGET | Usage::DEPTH_STENCIL) != 0
            {
                return Err(ResourceError::InvalidCall);
            }
            if let BufferKind::Index { format } = buffer.kind {
                if !matches!(format, Format::Index16 | Format::Index32) {
                    return Err(ResourceError::InvalidCall);
                }
                if usage.0 & (Usage::RT_PATCHES | Usage::NP_PATCHES) != 0 {
                    return Err(ResourceError::NotAvailable);
                }
            }
            layouts
                .try_reserve_exact(1)
                .map_err(|_| ResourceError::Allocation)?;
            layouts.push(SubresourceInfo {
                width: buffer.length,
                height: 1,
                depth: 1,
                row_pitch: buffer.length,
                slice_pitch: buffer.length,
                length: buffer.length,
                face: 0,
                level: 0,
            });
            total = buffer.length as usize;
        }
        ResourceDesc::Surface(surface) => {
            validate_top_level(surface.format, surface.width, surface.height)?;
            if surface.format.is_buffer()
                || surface.multisample == 1
                || surface.multisample > 16
                || (surface.multisample != 0 && surface.lockable)
                || usage.0 & !(Usage::RENDER_TARGET | Usage::DEPTH_STENCIL) != 0
                || usage.contains(Usage::RENDER_TARGET | Usage::DEPTH_STENCIL)
                || (surface.format.is_depth() != usage.contains(Usage::DEPTH_STENCIL))
                || (usage.0 != 0 && surface.pool != Pool::Default)
                || (surface.format.is_depth()
                    && surface.lockable
                    && surface.format != Format::D16Lockable)
            {
                return Err(ResourceError::InvalidCall);
            }
            layouts
                .try_reserve_exact(1)
                .map_err(|_| ResourceError::Allocation)?;
            let layout = image_layout(surface.format, surface.width, surface.height, 1, 0, 0)?;
            total = layout.length as usize;
            layouts.push(layout);
        }
        ResourceDesc::Texture(texture) => {
            validate_top_level(texture.format, texture.width, texture.height)?;
            if texture.width == 0
                || texture.height == 0
                || texture.depth == 0
                || texture.format.is_buffer()
                || usage.0 & !(Usage::RENDER_TARGET | Usage::DEPTH_STENCIL | Usage::DYNAMIC) != 0
                || usage.contains(Usage::RENDER_TARGET | Usage::DEPTH_STENCIL)
                || (texture.format.is_depth() != usage.contains(Usage::DEPTH_STENCIL))
                || (usage.0 & (Usage::RENDER_TARGET | Usage::DEPTH_STENCIL) != 0
                    && texture.pool != Pool::Default)
                || (texture.kind != TextureKind::Volume && texture.depth != 1)
                || (texture.kind == TextureKind::Cube && texture.width != texture.height)
                || (texture.kind == TextureKind::Volume
                    && usage.0 & (Usage::RENDER_TARGET | Usage::DEPTH_STENCIL) != 0)
            {
                return Err(ResourceError::InvalidCall);
            }
            let maximum = 32
                - texture
                    .width
                    .max(texture.height)
                    .max(texture.depth)
                    .leading_zeros();
            if texture.levels == 0 {
                texture.levels = maximum;
            }
            if texture.levels > maximum {
                return Err(ResourceError::InvalidCall);
            }
            let faces = if texture.kind == TextureKind::Cube {
                6
            } else {
                1
            };
            layouts
                .try_reserve_exact((faces * texture.levels) as usize)
                .map_err(|_| ResourceError::Allocation)?;
            for face in 0..faces {
                for level in 0..texture.levels {
                    let layout = image_layout(
                        texture.format,
                        (texture.width >> level).max(1),
                        (texture.height >> level).max(1),
                        (texture.depth >> level).max(1),
                        face,
                        level,
                    )?;
                    total = total
                        .checked_add(layout.length as usize)
                        .ok_or(ResourceError::ByteCapacity)?;
                    layouts.push(layout);
                }
            }
        }
    }
    Ok((desc, layouts, total))
}

fn validate_top_level(format: Format, width: u32, height: u32) -> Result<(), ResourceError> {
    if matches!(
        format,
        Format::Dxt1 | Format::Dxt2 | Format::Dxt3 | Format::Dxt4 | Format::Dxt5
    ) && (!width.is_multiple_of(4) || !height.is_multiple_of(4))
    {
        return Err(ResourceError::InvalidCall);
    }
    Ok(())
}

fn image_layout(
    format: Format,
    width: u32,
    height: u32,
    depth: u32,
    face: u32,
    level: u32,
) -> Result<SubresourceInfo, ResourceError> {
    if width == 0 || height == 0 || depth == 0 {
        return Err(ResourceError::InvalidCall);
    }
    let (block_width, block_height, block_bytes) = format.block();
    let row_pitch = width
        .div_ceil(block_width)
        .checked_mul(block_bytes)
        .ok_or(ResourceError::ByteCapacity)?;
    let slice_pitch = height
        .div_ceil(block_height)
        .checked_mul(row_pitch)
        .ok_or(ResourceError::ByteCapacity)?;
    let length = depth
        .checked_mul(slice_pitch)
        .ok_or(ResourceError::ByteCapacity)?;
    if row_pitch > i32::MAX as u32 || slice_pitch > i32::MAX as u32 {
        return Err(ResourceError::ByteCapacity);
    }
    Ok(SubresourceInfo {
        width,
        height,
        depth,
        row_pitch,
        slice_pitch,
        length,
        face,
        level,
    })
}
