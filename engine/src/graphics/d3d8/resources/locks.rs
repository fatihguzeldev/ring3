use super::storage::{LockedRegion, union};
use super::{
    Box3, LockFlags, LockInfo, Pool, ResourceDesc, ResourceError, ResourceStore, SurfaceRef, Usage,
    types::UnlockPlan,
};
use crate::graphics::ResourceId;

impl ResourceStore {
    pub fn can_unlock(
        &self,
        id: ResourceId,
        token: u64,
        bytes: Option<&[u8]>,
    ) -> Result<UnlockPlan, ResourceError> {
        let resource = self.resource(id)?;
        let lock = resource
            .subresources
            .iter()
            .flat_map(|subresource| &subresource.locks)
            .find(|lock| lock.info.token == token)
            .ok_or(ResourceError::InvalidCall)?;
        let readonly = lock.info.flags.contains(LockFlags::READ_ONLY);
        let version = if readonly {
            if bytes.is_some() {
                return Err(ResourceError::InvalidCall);
            }
            None
        } else {
            if bytes.is_none_or(|bytes| bytes.len() != lock.info.length as usize) {
                return Err(ResourceError::InvalidCall);
            }
            Some(
                resource
                    .version
                    .checked_add(1)
                    .ok_or(ResourceError::IdentityExhausted)?,
            )
        };
        Ok(UnlockPlan {
            info: lock.info,
            version,
            row_bytes: lock.row_bytes,
            rows: lock.rows,
            slices: lock.region.back - lock.region.front,
        })
    }
    pub fn lock_buffer(
        &mut self,
        id: ResourceId,
        offset: u32,
        size: u32,
        flags: LockFlags,
    ) -> Result<LockInfo, ResourceError> {
        let resource = self.resource(id)?;
        let ResourceDesc::Buffer(desc) = resource.desc else {
            return Err(ResourceError::InvalidCall);
        };
        validate_flags(flags, desc.usage, true)?;
        let length = if size == 0 {
            desc.length
                .checked_sub(offset)
                .ok_or(ResourceError::InvalidCall)?
        } else {
            size
        };
        if length == 0
            || offset
                .checked_add(length)
                .is_none_or(|end| end > desc.length)
        {
            return Err(ResourceError::InvalidCall);
        }
        if flags.contains(LockFlags::READ_ONLY) && desc.usage.contains(Usage::WRITE_ONLY) {
            return Err(ResourceError::InvalidCall);
        }
        if !resource.subresources[0].cpu_current && !flags.contains(LockFlags::DISCARD) {
            return Err(ResourceError::ReadbackRequired);
        }
        let region = Box3 {
            left: offset,
            top: 0,
            front: 0,
            right: offset + length,
            bottom: 1,
            back: 1,
        };
        self.insert_lock(
            id,
            LockedRegion {
                info: LockInfo {
                    token: 0,
                    subresource: 0,
                    offset,
                    row_pitch: desc.length,
                    slice_pitch: desc.length,
                    length,
                    flags,
                },
                region,
                row_bytes: length,
                rows: 1,
            },
        )
    }

    pub fn lock_box(
        &mut self,
        surface: SurfaceRef,
        region: Option<Box3>,
        flags: LockFlags,
    ) -> Result<LockInfo, ResourceError> {
        let resource = self.resource(surface.resource)?;
        if matches!(resource.desc, ResourceDesc::Buffer(_)) {
            return Err(ResourceError::InvalidCall);
        }
        validate_flags(flags, resource.desc.usage(), false)?;
        if flags.contains(LockFlags::NO_OVERWRITE) {
            return Err(ResourceError::InvalidCall);
        }
        match resource.desc {
            ResourceDesc::Texture(desc)
                if desc.pool == Pool::Default && !desc.usage.contains(Usage::DYNAMIC) =>
            {
                return Err(ResourceError::InvalidCall);
            }
            ResourceDesc::Surface(desc) if !desc.lockable || desc.multisample != 0 => {
                return Err(ResourceError::InvalidCall);
            }
            _ => {}
        }
        let subresource = self.subresource(surface)?;
        if !subresource.locks.is_empty() {
            return Err(ResourceError::InvalidCall);
        }
        let info = subresource.info;
        let full = Box3 {
            left: 0,
            top: 0,
            front: 0,
            right: info.width,
            bottom: info.height,
            back: info.depth,
        };
        let region = region.unwrap_or(full);
        validate_box(region, info.width, info.height, info.depth)?;
        if flags.contains(LockFlags::DISCARD) && region != full {
            return Err(ResourceError::InvalidCall);
        }
        if !subresource.cpu_current && !flags.contains(LockFlags::DISCARD) {
            return Err(ResourceError::ReadbackRequired);
        }
        let (block_width, block_height, block_bytes) = resource.desc.format().block();
        validate_alignment(region, info.width, info.height, block_width, block_height)?;
        let row_bytes = (region.right - region.left).div_ceil(block_width) * block_bytes;
        let rows = (region.bottom - region.top).div_ceil(block_height);
        let offset = region.front * info.slice_pitch
            + region.top / block_height * info.row_pitch
            + region.left / block_width * block_bytes;
        let length = (region.back - region.front - 1) * info.slice_pitch
            + (rows - 1) * info.row_pitch
            + row_bytes;
        self.insert_lock(
            surface.resource,
            LockedRegion {
                info: LockInfo {
                    token: 0,
                    subresource: surface.subresource,
                    offset,
                    row_pitch: info.row_pitch,
                    slice_pitch: info.slice_pitch,
                    length,
                    flags,
                },
                region,
                row_bytes,
                rows,
            },
        )
    }

    pub fn lock_bytes(&self, id: ResourceId, token: u64) -> Result<&[u8], ResourceError> {
        let resource = self.resource(id)?;
        for subresource in &resource.subresources {
            if let Some(lock) = subresource
                .locks
                .iter()
                .find(|lock| lock.info.token == token)
            {
                if !subresource.cpu_current && !lock.info.flags.contains(LockFlags::DISCARD) {
                    return Err(ResourceError::ReadbackRequired);
                }
                return Ok(&subresource.bytes
                    [lock.info.offset as usize..(lock.info.offset + lock.info.length) as usize]);
            }
        }
        Err(ResourceError::InvalidCall)
    }

    pub fn unlock(
        &mut self,
        id: ResourceId,
        token: u64,
        bytes: Option<&[u8]>,
    ) -> Result<Option<u64>, ResourceError> {
        let resource = self.resource_mut(id)?;
        let (subresource_index, lock_index) = resource
            .subresources
            .iter()
            .enumerate()
            .find_map(|(subresource_index, subresource)| {
                subresource
                    .locks
                    .iter()
                    .position(|lock| lock.info.token == token)
                    .map(|lock_index| (subresource_index, lock_index))
            })
            .ok_or(ResourceError::InvalidCall)?;
        let lock = resource.subresources[subresource_index].locks[lock_index];
        let readonly = lock.info.flags.contains(LockFlags::READ_ONLY);
        if readonly {
            if bytes.is_some() {
                return Err(ResourceError::InvalidCall);
            }
            resource.subresources[subresource_index]
                .locks
                .remove(lock_index);
            return Ok(None);
        }
        let bytes = bytes
            .filter(|bytes| bytes.len() == lock.info.length as usize)
            .ok_or(ResourceError::InvalidCall)?;
        let version = resource
            .version
            .checked_add(1)
            .ok_or(ResourceError::IdentityExhausted)?;
        let subresource = &mut resource.subresources[subresource_index];
        if lock.info.flags.contains(LockFlags::DISCARD) {
            subresource.bytes.fill(0);
        }
        for slice in 0..(lock.region.back - lock.region.front) {
            for row in 0..lock.rows {
                let input = (slice * lock.info.slice_pitch + row * lock.info.row_pitch) as usize;
                let output = lock.info.offset as usize + input;
                subresource.bytes[output..output + lock.row_bytes as usize]
                    .copy_from_slice(&bytes[input..input + lock.row_bytes as usize]);
            }
        }
        subresource.cpu_current = true;
        subresource.locks.remove(lock_index);
        if matches!(resource.desc, ResourceDesc::Texture(_))
            && subresource.info.level == 0
            && !lock.info.flags.contains(LockFlags::NO_DIRTY_UPDATE)
        {
            let dirty = &mut resource.dirty[subresource.info.face as usize];
            *dirty = Some(match *dirty {
                Some(old) => union(old, lock.region),
                None => lock.region,
            });
        }
        resource.version = version;
        Ok(Some(version))
    }

    fn insert_lock(
        &mut self,
        id: ResourceId,
        mut lock: LockedRegion,
    ) -> Result<LockInfo, ResourceError> {
        let token = self
            .lock_sequence
            .checked_add(1)
            .ok_or(ResourceError::IdentityExhausted)?;
        lock.info.token = token;
        let resource = self.resource_mut(id)?;
        let subresource = &mut resource.subresources[lock.info.subresource as usize];
        subresource
            .locks
            .try_reserve(1)
            .map_err(|_| ResourceError::Allocation)?;
        subresource.locks.push(lock);
        self.lock_sequence = token;
        Ok(lock.info)
    }
}

fn validate_flags(flags: LockFlags, usage: Usage, buffer: bool) -> Result<(), ResourceError> {
    if flags.0 & !LockFlags::KNOWN != 0
        || (flags.contains(LockFlags::READ_ONLY)
            && flags.0 & (LockFlags::DISCARD | LockFlags::NO_OVERWRITE) != 0)
        || (flags.0 & (LockFlags::DISCARD | LockFlags::NO_OVERWRITE) != 0
            && !usage.contains(Usage::DYNAMIC))
        || (flags.contains(LockFlags::DISCARD | LockFlags::NO_OVERWRITE))
        || (buffer && flags.contains(LockFlags::NO_DIRTY_UPDATE))
    {
        return Err(ResourceError::InvalidCall);
    }
    Ok(())
}

pub(super) fn validate_box(
    region: Box3,
    width: u32,
    height: u32,
    depth: u32,
) -> Result<(), ResourceError> {
    if region.left >= region.right
        || region.top >= region.bottom
        || region.front >= region.back
        || region.right > width
        || region.bottom > height
        || region.back > depth
    {
        return Err(ResourceError::InvalidCall);
    }
    Ok(())
}

pub(super) fn validate_alignment(
    region: Box3,
    width: u32,
    height: u32,
    block_width: u32,
    block_height: u32,
) -> Result<(), ResourceError> {
    if !region.left.is_multiple_of(block_width)
        || !region.top.is_multiple_of(block_height)
        || (!region.right.is_multiple_of(block_width) && region.right != width)
        || (!region.bottom.is_multiple_of(block_height) && region.bottom != height)
    {
        return Err(ResourceError::InvalidCall);
    }
    Ok(())
}
