use super::{Box3, Pool, Rect, ResourceDesc, ResourceError, ResourceStore, SurfaceRef};
use super::{
    locks::{validate_alignment, validate_box},
    storage::union,
};
use crate::graphics::ResourceId;

#[derive(Clone, Copy)]
struct CopyRegion {
    source: usize,
    destination: usize,
    row_bytes: usize,
    rows: usize,
    source_pitch: usize,
    destination_pitch: usize,
    dirty: Box3,
}

pub struct PreparedCopy {
    source: SurfaceRef,
    destination: SurfaceRef,
    source_version: u64,
    destination_version: u64,
    copies: Vec<CopyRegion>,
}

impl PreparedCopy {
    pub fn destination_version(&self) -> u64 {
        self.destination_version
    }
}

impl ResourceStore {
    pub fn can_update_texture(
        &self,
        source: ResourceId,
        destination: ResourceId,
    ) -> Result<bool, ResourceError> {
        if source == destination {
            return Err(ResourceError::InvalidCall);
        }
        Ok(update_plan(self.resource(source)?, self.resource(destination)?)?.is_some())
    }

    pub fn update_texture(
        &mut self,
        source: ResourceId,
        destination: ResourceId,
    ) -> Result<bool, ResourceError> {
        let (source, destination) = self.pair_mut(source, destination)?;
        let Some(UpdatePlan {
            source_desc,
            destination_desc,
            level_offset,
            faces,
            version,
        }) = update_plan(source, destination)?
        else {
            return Ok(false);
        };
        for face in 0..faces {
            if source.dirty[face as usize].is_none() {
                continue;
            }
            for level in 0..destination_desc.levels {
                let input = &source.subresources
                    [(face * source_desc.levels + level_offset + level) as usize];
                let output = &mut destination.subresources
                    [(face * destination_desc.levels + level) as usize];
                output.bytes.copy_from_slice(&input.bytes);
                output.cpu_current = true;
            }
        }
        source.dirty.fill(None);
        destination.version = version;
        Ok(true)
    }

    pub fn prepare_copy_rects(
        &self,
        source: SurfaceRef,
        destination: SurfaceRef,
        rectangles: &[Rect],
        points: Option<&[(u32, u32)]>,
    ) -> Result<PreparedCopy, ResourceError> {
        if source == destination {
            return Err(ResourceError::InvalidCall);
        }
        let source_resource = self.resource(source.resource)?;
        let destination_resource = self.resource(destination.resource)?;
        let format = source_resource.desc.format();
        if format != destination_resource.desc.format()
            || format.is_depth()
            || matches!(source_resource.desc, ResourceDesc::Buffer(_))
            || matches!(destination_resource.desc, ResourceDesc::Buffer(_))
        {
            return Err(ResourceError::InvalidCall);
        }
        let input = self.subresource(source)?;
        let output = self.subresource(destination)?;
        if input.info.depth != 1
            || output.info.depth != 1
            || !input.locks.is_empty()
            || !output.locks.is_empty()
        {
            return Err(ResourceError::InvalidCall);
        }
        if !input.cpu_current || !output.cpu_current {
            return Err(ResourceError::ReadbackRequired);
        }
        let count = rectangles.len().max(1);
        if points.is_some_and(|points| points.len() != count) {
            return Err(ResourceError::InvalidCall);
        }
        let version = destination_resource
            .version
            .checked_add(1)
            .ok_or(ResourceError::IdentityExhausted)?;
        let (block_width, block_height, block_bytes) = format.block();
        let mut copies = Vec::new();
        copies
            .try_reserve_exact(count)
            .map_err(|_| ResourceError::Allocation)?;
        for index in 0..count {
            let rectangle = rectangles.get(index).copied().unwrap_or(Rect {
                left: 0,
                top: 0,
                right: input.info.width,
                bottom: input.info.height,
            });
            let source_box = Box3::from(rectangle);
            validate_box(source_box, input.info.width, input.info.height, 1)?;
            validate_alignment(
                source_box,
                input.info.width,
                input.info.height,
                block_width,
                block_height,
            )?;
            let (x, y) = points.map_or((rectangle.left, rectangle.top), |points| points[index]);
            let width = rectangle.right - rectangle.left;
            let height = rectangle.bottom - rectangle.top;
            let destination_box = Box3 {
                left: x,
                top: y,
                front: 0,
                right: x.checked_add(width).ok_or(ResourceError::InvalidCall)?,
                bottom: y.checked_add(height).ok_or(ResourceError::InvalidCall)?,
                back: 1,
            };
            validate_box(destination_box, output.info.width, output.info.height, 1)?;
            validate_alignment(
                destination_box,
                output.info.width,
                output.info.height,
                block_width,
                block_height,
            )?;
            copies.push(CopyRegion {
                source: (rectangle.top / block_height * input.info.row_pitch
                    + rectangle.left / block_width * block_bytes) as usize,
                destination: (y / block_height * output.info.row_pitch
                    + x / block_width * block_bytes) as usize,
                row_bytes: (width.div_ceil(block_width) * block_bytes) as usize,
                rows: height.div_ceil(block_height) as usize,
                source_pitch: input.info.row_pitch as usize,
                destination_pitch: output.info.row_pitch as usize,
                dirty: destination_box,
            });
        }
        Ok(PreparedCopy {
            source,
            destination,
            source_version: source_resource.version,
            destination_version: version,
            copies,
        })
    }

    pub fn can_commit_copy(&self, prepared: &PreparedCopy) -> Result<(), ResourceError> {
        if self.version(prepared.source.resource)? != prepared.source_version
            || self.version(prepared.destination.resource)?.checked_add(1)
                != Some(prepared.destination_version)
        {
            return Err(ResourceError::StaleResource);
        }
        for surface in [prepared.source, prepared.destination] {
            let subresource = self.subresource(surface)?;
            if !subresource.locks.is_empty() {
                return Err(ResourceError::InvalidCall);
            }
            if !subresource.cpu_current {
                return Err(ResourceError::ReadbackRequired);
            }
        }
        Ok(())
    }

    pub fn copy_rects(
        &mut self,
        source: SurfaceRef,
        destination: SurfaceRef,
        rectangles: &[Rect],
        points: Option<&[(u32, u32)]>,
    ) -> Result<(), ResourceError> {
        let prepared = self.prepare_copy_rects(source, destination, rectangles, points)?;
        self.commit_copy(prepared)
    }

    pub fn commit_copy(&mut self, prepared: PreparedCopy) -> Result<(), ResourceError> {
        self.can_commit_copy(&prepared)?;
        let PreparedCopy {
            source,
            destination,
            destination_version: version,
            copies,
            ..
        } = prepared;
        {
            let (input, output) = self.subresources_mut(source, destination)?;
            for copy in &copies {
                for row in 0..copy.rows {
                    let start = copy.source + row * copy.source_pitch;
                    let destination_start = copy.destination + row * copy.destination_pitch;
                    output.bytes[destination_start..destination_start + copy.row_bytes]
                        .copy_from_slice(&input.bytes[start..start + copy.row_bytes]);
                }
            }
        }
        let destination_resource = self.resource_mut(destination.resource)?;
        let info = destination_resource.subresources[destination.subresource as usize].info;
        if matches!(destination_resource.desc, ResourceDesc::Texture(_)) && info.level == 0 {
            let dirty = &mut destination_resource.dirty[info.face as usize];
            for copy in copies {
                *dirty = Some(match *dirty {
                    Some(old) => union(old, copy.dirty),
                    None => copy.dirty,
                });
            }
        }
        destination_resource.version = version;
        Ok(())
    }
}

struct UpdatePlan {
    source_desc: super::TextureDesc,
    destination_desc: super::TextureDesc,
    level_offset: u32,
    faces: u32,
    version: u64,
}

fn update_plan(
    source: &super::storage::Resource,
    destination: &super::storage::Resource,
) -> Result<Option<UpdatePlan>, ResourceError> {
    let (ResourceDesc::Texture(source_desc), ResourceDesc::Texture(destination_desc)) =
        (source.desc, destination.desc)
    else {
        return Err(ResourceError::InvalidCall);
    };
    if source_desc.pool != Pool::SystemMemory
        || destination_desc.pool != Pool::Default
        || source_desc.kind != destination_desc.kind
        || source_desc.format != destination_desc.format
        || source_desc.levels < destination_desc.levels
        || source
            .subresources
            .iter()
            .chain(&destination.subresources)
            .any(|subresource| !subresource.locks.is_empty())
    {
        return Err(ResourceError::InvalidCall);
    }
    let level_offset = source_desc.levels - destination_desc.levels;
    let faces = source.dirty.len() as u32;
    for face in 0..faces {
        for level in 0..destination_desc.levels {
            let input =
                &source.subresources[(face * source_desc.levels + level_offset + level) as usize];
            let output =
                &destination.subresources[(face * destination_desc.levels + level) as usize];
            if (input.info.width, input.info.height, input.info.depth)
                != (output.info.width, output.info.height, output.info.depth)
            {
                return Err(ResourceError::InvalidCall);
            }
            if !input.cpu_current {
                return Err(ResourceError::ReadbackRequired);
            }
        }
    }
    if !source.dirty.iter().any(Option::is_some) {
        return Ok(None);
    }
    let version = destination
        .version
        .checked_add(1)
        .ok_or(ResourceError::IdentityExhausted)?;
    Ok(Some(UpdatePlan {
        source_desc,
        destination_desc,
        level_offset,
        faces,
        version,
    }))
}
