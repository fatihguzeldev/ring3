use super::{
    Box3, ResourceDesc, ResourceError, ResourceStore, SubresourceInfo, TextureKind, layout,
    storage::{Resource, Slot, Subresource},
};
use crate::graphics::ResourceId;

pub struct PreparedReplacement {
    epoch: u64,
    bytes_before: usize,
    bytes_after: usize,
    removed: Vec<ResourceId>,
    additions: Vec<Resource>,
    ids: Vec<ResourceId>,
    infos: Vec<Vec<SubresourceInfo>>,
}

impl PreparedReplacement {
    pub fn ids(&self) -> &[ResourceId] {
        &self.ids
    }
    pub fn desc(&self, index: usize) -> ResourceDesc {
        self.additions[index].desc
    }
    pub fn infos(&self, index: usize) -> &[SubresourceInfo] {
        &self.infos[index]
    }
}

pub struct PreparedResource(PreparedReplacement);

impl PreparedResource {
    pub fn id(&self) -> ResourceId {
        self.0.ids[0]
    }
    pub fn desc(&self) -> ResourceDesc {
        self.0.desc(0)
    }
    pub fn infos(&self) -> &[SubresourceInfo] {
        self.0.infos(0)
    }
}

impl ResourceStore {
    pub fn prepare_create(&self, desc: ResourceDesc) -> Result<PreparedResource, ResourceError> {
        Ok(PreparedResource(self.prepare_replace(&[], &[desc])?))
    }
    pub fn can_commit_create(&self, prepared: &PreparedResource) -> Result<(), ResourceError> {
        self.can_commit_replace(&prepared.0)
    }
    pub fn commit_create(
        &mut self,
        prepared: PreparedResource,
    ) -> Result<ResourceId, ResourceError> {
        let id = prepared.id();
        self.commit_replace(prepared.0)?;
        Ok(id)
    }

    pub fn prepare_replace(
        &self,
        old: &[ResourceId],
        new: &[ResourceDesc],
    ) -> Result<PreparedReplacement, ResourceError> {
        if old.len() > self.slots.len() || new.len() > self.slots.len() {
            return Err(ResourceError::ResourceCapacity);
        }
        let mut total = self.bytes;
        for (index, id) in old.iter().enumerate() {
            if old[..index].contains(id) || self.is_locked(*id)? {
                return Err(ResourceError::InvalidCall);
            }
            total -= self.resource(*id)?.bytes;
        }
        let mut layouts = Vec::new();
        layouts
            .try_reserve_exact(new.len())
            .map_err(|_| ResourceError::Allocation)?;
        for desc in new {
            let (desc, infos, bytes) = layout::describe(*desc)?;
            total = total
                .checked_add(bytes)
                .filter(|total| *total <= self.limits.resource_bytes as usize)
                .ok_or(ResourceError::ByteCapacity)?;
            layouts.push((desc, infos, bytes));
        }
        let mut ids = Vec::new();
        ids.try_reserve_exact(new.len())
            .map_err(|_| ResourceError::Allocation)?;
        for (index, slot) in self.slots.iter().enumerate() {
            if ids.len() == new.len() {
                break;
            }
            if (slot.resource.is_none() || old.iter().any(|id| id.slot as usize == index))
                && slot.generation != u32::MAX
            {
                ids.push(ResourceId {
                    epoch: self.epoch,
                    slot: index as u32,
                    generation: slot.generation + 1,
                });
            }
        }
        if ids.len() != new.len() {
            return Err(
                if self
                    .slots
                    .iter()
                    .any(|slot| slot.generation == u32::MAX && slot.resource.is_none())
                {
                    ResourceError::IdentityExhausted
                } else {
                    ResourceError::ResourceCapacity
                },
            );
        }
        let mut removed = Vec::new();
        removed
            .try_reserve_exact(old.len())
            .map_err(|_| ResourceError::Allocation)?;
        removed.extend_from_slice(old);
        let mut additions = Vec::new();
        let mut infos = Vec::new();
        additions
            .try_reserve_exact(layouts.len())
            .map_err(|_| ResourceError::Allocation)?;
        infos
            .try_reserve_exact(layouts.len())
            .map_err(|_| ResourceError::Allocation)?;
        for (desc, layout, bytes) in layouts {
            additions.push(allocate(desc, &layout, bytes)?);
            infos.push(layout);
        }
        Ok(PreparedReplacement {
            epoch: self.epoch,
            bytes_before: self.bytes,
            bytes_after: total,
            removed,
            additions,
            ids,
            infos,
        })
    }

    pub fn can_commit_replace(&self, prepared: &PreparedReplacement) -> Result<(), ResourceError> {
        if prepared.epoch != self.epoch || prepared.bytes_before != self.bytes {
            return Err(ResourceError::StaleResource);
        }
        for id in &prepared.removed {
            self.resource(*id)?;
            if self.is_locked(*id)? {
                return Err(ResourceError::InvalidCall);
            }
        }
        for id in &prepared.ids {
            let slot = &self.slots[id.slot as usize];
            if slot.generation.checked_add(1) != Some(id.generation)
                || (slot.resource.is_some()
                    && !prepared.removed.iter().any(|old| old.slot == id.slot))
            {
                return Err(ResourceError::StaleResource);
            }
        }
        Ok(())
    }

    pub fn commit_replace(&mut self, prepared: PreparedReplacement) -> Result<(), ResourceError> {
        self.can_commit_replace(&prepared)?;
        for id in prepared.removed {
            self.slots[id.slot as usize].resource = None;
        }
        for (id, resource) in prepared.ids.into_iter().zip(prepared.additions) {
            self.slots[id.slot as usize] = Slot {
                generation: id.generation,
                resource: Some(resource),
            };
        }
        self.bytes = prepared.bytes_after;
        Ok(())
    }
}

fn allocate(
    desc: ResourceDesc,
    layouts: &[SubresourceInfo],
    bytes: usize,
) -> Result<Resource, ResourceError> {
    let mut subresources = Vec::new();
    subresources
        .try_reserve_exact(layouts.len())
        .map_err(|_| ResourceError::Allocation)?;
    for info in layouts {
        let mut data = Vec::new();
        data.try_reserve_exact(info.length as usize)
            .map_err(|_| ResourceError::Allocation)?;
        data.resize(info.length as usize, 0);
        subresources.push(Subresource {
            info: *info,
            bytes: data,
            locks: Vec::new(),
            cpu_current: true,
        });
    }
    let faces = match desc {
        ResourceDesc::Texture(texture) if texture.kind == TextureKind::Cube => 6,
        _ => 1,
    };
    let mut dirty = Vec::new();
    dirty
        .try_reserve_exact(faces)
        .map_err(|_| ResourceError::Allocation)?;
    let full = match desc {
        ResourceDesc::Texture(texture) => Some(Box3 {
            left: 0,
            top: 0,
            front: 0,
            right: texture.width,
            bottom: texture.height,
            back: texture.depth,
        }),
        _ => None,
    };
    dirty.resize(faces, full);
    Ok(Resource {
        desc,
        subresources,
        version: 1,
        priority: 0,
        lod: 0,
        dirty,
        bytes,
        private_data: Vec::new(),
        private_revision: 1,
    })
}
