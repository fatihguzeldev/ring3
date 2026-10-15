use super::{
    Box3, LockInfo, Pool, ResourceDesc, ResourceError, SubresourceInfo, SurfaceRef, layout,
};
use crate::graphics::{GraphicsLimits, ResourceId};

pub(super) struct Subresource {
    pub info: SubresourceInfo,
    pub bytes: Vec<u8>,
    pub locks: Vec<LockedRegion>,
    pub cpu_current: bool,
}

#[derive(Clone, Copy)]
pub(super) struct LockedRegion {
    pub info: LockInfo,
    pub region: Box3,
    pub row_bytes: u32,
    pub rows: u32,
}

pub(super) struct Resource {
    pub desc: ResourceDesc,
    pub subresources: Vec<Subresource>,
    pub version: u64,
    pub priority: u32,
    pub lod: u32,
    pub dirty: Vec<Option<Box3>>,
    pub bytes: usize,
    pub private_data: Vec<([u8; 16], Vec<u8>)>,
}

#[derive(Default)]
pub(super) struct Slot {
    pub generation: u32,
    pub resource: Option<Resource>,
}

pub struct ResourceStore {
    pub(super) epoch: u64,
    pub(super) limits: GraphicsLimits,
    pub(super) slots: Vec<Slot>,
    pub(super) bytes: usize,
    pub(super) lock_sequence: u64,
}

impl ResourceStore {
    pub fn new(epoch: u64, limits: GraphicsLimits) -> Result<Self, ResourceError> {
        if epoch == 0
            || limits.resources == 0
            || limits.resources > GraphicsLimits::MAX_RESOURCES
            || limits.resource_bytes == 0
            || limits.resource_bytes > GraphicsLimits::MAX_RESOURCE_BYTES
        {
            return Err(ResourceError::InvalidCall);
        }
        let mut slots = Vec::new();
        slots
            .try_reserve_exact(limits.resources as usize)
            .map_err(|_| ResourceError::Allocation)?;
        slots.resize_with(limits.resources as usize, Slot::default);
        Ok(Self {
            epoch,
            limits,
            slots,
            bytes: 0,
            lock_sequence: 0,
        })
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }
    pub fn resource_bytes(&self) -> usize {
        self.bytes
    }

    pub fn ids(&self) -> impl Iterator<Item = ResourceId> + '_ {
        self.slots.iter().enumerate().filter_map(|(index, slot)| {
            slot.resource.as_ref().map(|_| ResourceId {
                epoch: self.epoch,
                slot: index as u32,
                generation: slot.generation,
            })
        })
    }

    pub fn allocation_size(desc: ResourceDesc) -> Result<usize, ResourceError> {
        Ok(layout::describe(desc)?.2)
    }

    pub fn create(&mut self, desc: ResourceDesc) -> Result<ResourceId, ResourceError> {
        let prepared = self.prepare_create(desc)?;
        self.commit_create(prepared)
    }

    pub fn destroy(&mut self, id: ResourceId) -> Result<(), ResourceError> {
        let index = self.index(id)?;
        self.bytes -= self.slots[index].resource.take().unwrap().bytes;
        Ok(())
    }

    pub fn desc(&self, id: ResourceId) -> Result<ResourceDesc, ResourceError> {
        Ok(self.resource(id)?.desc)
    }
    pub fn version(&self, id: ResourceId) -> Result<u64, ResourceError> {
        Ok(self.resource(id)?.version)
    }
    pub fn subresource_count(&self, id: ResourceId) -> Result<u32, ResourceError> {
        Ok(self.resource(id)?.subresources.len() as u32)
    }
    pub fn info(&self, surface: SurfaceRef) -> Result<SubresourceInfo, ResourceError> {
        Ok(self.subresource(surface)?.info)
    }

    pub fn bytes(&self, surface: SurfaceRef) -> Result<&[u8], ResourceError> {
        let subresource = self.subresource(surface)?;
        if !subresource.cpu_current {
            return Err(ResourceError::ReadbackRequired);
        }
        Ok(&subresource.bytes)
    }

    pub fn is_locked(&self, id: ResourceId) -> Result<bool, ResourceError> {
        Ok(self
            .resource(id)?
            .subresources
            .iter()
            .any(|subresource| !subresource.locks.is_empty()))
    }

    pub fn is_dirty(&self, id: ResourceId) -> Result<bool, ResourceError> {
        Ok(self.resource(id)?.dirty.iter().any(Option::is_some))
    }

    pub fn add_dirty(
        &mut self,
        id: ResourceId,
        face: u32,
        region: Option<Box3>,
    ) -> Result<(), ResourceError> {
        let resource = self.resource_mut(id)?;
        let ResourceDesc::Texture(desc) = resource.desc else {
            return Err(ResourceError::InvalidCall);
        };
        let full = Box3 {
            left: 0,
            top: 0,
            front: 0,
            right: desc.width,
            bottom: desc.height,
            back: desc.depth,
        };
        let region = region.unwrap_or(full);
        super::locks::validate_box(region, full.right, full.bottom, full.back)?;
        let dirty = resource
            .dirty
            .get_mut(face as usize)
            .ok_or(ResourceError::InvalidCall)?;
        *dirty = Some(match *dirty {
            Some(old) => union(old, region),
            None => region,
        });
        Ok(())
    }

    pub fn priority(&self, id: ResourceId) -> Result<u32, ResourceError> {
        Ok(self.resource(id)?.priority)
    }
    pub fn set_priority(&mut self, id: ResourceId, value: u32) -> Result<u32, ResourceError> {
        let resource = self.resource_mut(id)?;
        Ok(std::mem::replace(&mut resource.priority, value))
    }

    pub fn lod(&self, id: ResourceId) -> Result<u32, ResourceError> {
        Ok(self.resource(id)?.lod)
    }
    pub fn set_lod(&mut self, id: ResourceId, value: u32) -> Result<u32, ResourceError> {
        let resource = self.resource_mut(id)?;
        let ResourceDesc::Texture(desc) = resource.desc else {
            return Err(ResourceError::InvalidCall);
        };
        if desc.pool != Pool::Managed {
            return Ok(0);
        }
        Ok(std::mem::replace(
            &mut resource.lod,
            value.min(desc.levels - 1),
        ))
    }

    pub fn can_reset_default(&self) -> Result<(), ResourceError> {
        self.can_reset_default_except(&[])
    }

    pub fn can_reset_default_except(&self, implicit: &[ResourceId]) -> Result<(), ResourceError> {
        for id in implicit {
            if self.is_locked(*id)? {
                return Err(ResourceError::InvalidCall);
            }
        }
        if self.ids().any(|id| {
            self.resource(id)
                .is_ok_and(|resource| resource.desc.pool() == Pool::Default)
                && !implicit.contains(&id)
        }) {
            return Err(ResourceError::InvalidCall);
        }
        Ok(())
    }

    pub fn can_mark_gpu_written(&self, surface: SurfaceRef) -> Result<(), ResourceError> {
        let resource = self.resource(surface.resource)?;
        let subresource = self.subresource(surface)?;
        if !subresource.locks.is_empty() {
            return Err(ResourceError::InvalidCall);
        }
        if resource.version == u64::MAX {
            return Err(ResourceError::IdentityExhausted);
        }
        Ok(())
    }

    pub fn mark_gpu_written(&mut self, surface: SurfaceRef) -> Result<(), ResourceError> {
        self.can_mark_gpu_written(surface)?;
        let resource = self.resource_mut(surface.resource)?;
        resource.version += 1;
        resource.subresources[surface.subresource as usize].cpu_current = false;
        Ok(())
    }

    pub fn complete_readback(
        &mut self,
        surface: SurfaceRef,
        version: u64,
        bytes: &[u8],
    ) -> Result<(), ResourceError> {
        let resource = self.resource_mut(surface.resource)?;
        if resource.version != version {
            return Err(ResourceError::StaleResource);
        }
        let subresource = resource
            .subresources
            .get_mut(surface.subresource as usize)
            .ok_or(ResourceError::InvalidCall)?;
        if subresource.cpu_current
            || !subresource.locks.is_empty()
            || bytes.len() != subresource.bytes.len()
        {
            return Err(ResourceError::InvalidCall);
        }
        subresource.bytes.copy_from_slice(bytes);
        subresource.cpu_current = true;
        Ok(())
    }

    pub(super) fn resource(&self, id: ResourceId) -> Result<&Resource, ResourceError> {
        Ok(self.slots[self.index(id)?].resource.as_ref().unwrap())
    }

    pub(super) fn resource_mut(&mut self, id: ResourceId) -> Result<&mut Resource, ResourceError> {
        let index = self.index(id)?;
        Ok(self.slots[index].resource.as_mut().unwrap())
    }

    pub(super) fn subresource(&self, surface: SurfaceRef) -> Result<&Subresource, ResourceError> {
        self.resource(surface.resource)?
            .subresources
            .get(surface.subresource as usize)
            .ok_or(ResourceError::InvalidCall)
    }

    pub(super) fn pair_mut(
        &mut self,
        source: ResourceId,
        destination: ResourceId,
    ) -> Result<(&mut Resource, &mut Resource), ResourceError> {
        let source_index = self.index(source)?;
        let destination_index = self.index(destination)?;
        if source_index == destination_index {
            return Err(ResourceError::InvalidCall);
        }
        if source_index < destination_index {
            let (left, right) = self.slots.split_at_mut(destination_index);
            Ok((
                left[source_index].resource.as_mut().unwrap(),
                right[0].resource.as_mut().unwrap(),
            ))
        } else {
            let (left, right) = self.slots.split_at_mut(source_index);
            Ok((
                right[0].resource.as_mut().unwrap(),
                left[destination_index].resource.as_mut().unwrap(),
            ))
        }
    }

    pub(super) fn subresources_mut(
        &mut self,
        source: SurfaceRef,
        destination: SurfaceRef,
    ) -> Result<(&mut Subresource, &mut Subresource), ResourceError> {
        if source.resource == destination.resource {
            let resource = self.resource_mut(source.resource)?;
            let source_index = source.subresource as usize;
            let destination_index = destination.subresource as usize;
            if source_index == destination_index
                || source_index.max(destination_index) >= resource.subresources.len()
            {
                return Err(ResourceError::InvalidCall);
            }
            if source_index < destination_index {
                let (left, right) = resource.subresources.split_at_mut(destination_index);
                Ok((&mut left[source_index], &mut right[0]))
            } else {
                let (left, right) = resource.subresources.split_at_mut(source_index);
                Ok((&mut right[0], &mut left[destination_index]))
            }
        } else {
            let (source_resource, destination_resource) =
                self.pair_mut(source.resource, destination.resource)?;
            let source = source_resource
                .subresources
                .get_mut(source.subresource as usize)
                .ok_or(ResourceError::InvalidCall)?;
            let destination = destination_resource
                .subresources
                .get_mut(destination.subresource as usize)
                .ok_or(ResourceError::InvalidCall)?;
            Ok((source, destination))
        }
    }

    fn index(&self, id: ResourceId) -> Result<usize, ResourceError> {
        let index = id.slot as usize;
        if id.epoch != self.epoch
            || id.generation == 0
            || !self
                .slots
                .get(index)
                .is_some_and(|slot| slot.generation == id.generation && slot.resource.is_some())
        {
            return Err(ResourceError::StaleResource);
        }
        Ok(index)
    }
}

pub(super) fn union(old: Box3, new: Box3) -> Box3 {
    Box3 {
        left: old.left.min(new.left),
        top: old.top.min(new.top),
        front: old.front.min(new.front),
        right: old.right.max(new.right),
        bottom: old.bottom.max(new.bottom),
        back: old.back.max(new.back),
    }
}
