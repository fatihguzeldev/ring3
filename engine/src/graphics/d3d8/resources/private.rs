use super::{ResourceError, ResourceStore};
use crate::graphics::ResourceId;

const KEY_CHARGE: usize = 16;

pub(super) struct Entry {
    namespace: u64,
    key: [u8; 16],
    bytes: Vec<u8>,
}

pub struct PreparedPrivateData {
    id: ResourceId,
    namespace: u64,
    key: [u8; 16],
    index: Option<usize>,
    revision: u64,
    next_revision: u64,
    bytes_before: usize,
    bytes_after: usize,
    previous_charge: usize,
    charge: usize,
    bytes: Vec<u8>,
}

impl ResourceStore {
    pub fn private_data(&self, id: ResourceId, key: [u8; 16]) -> Result<&[u8], ResourceError> {
        self.private_data_for(id, 0, key)
    }

    pub fn private_data_for(
        &self,
        id: ResourceId,
        namespace: u64,
        key: [u8; 16],
    ) -> Result<&[u8], ResourceError> {
        self.resource(id)?
            .private_data
            .iter()
            .find(|entry| entry.namespace == namespace && entry.key == key)
            .map(|entry| entry.bytes.as_slice())
            .ok_or(ResourceError::NotFound)
    }

    pub fn set_private_data(
        &mut self,
        id: ResourceId,
        key: [u8; 16],
        bytes: &[u8],
    ) -> Result<(), ResourceError> {
        self.set_private_data_for(id, 0, key, bytes)
    }

    pub fn set_private_data_for(
        &mut self,
        id: ResourceId,
        namespace: u64,
        key: [u8; 16],
        bytes: &[u8],
    ) -> Result<(), ResourceError> {
        let prepared = self.prepare_private_data(id, namespace, key, bytes)?;
        self.commit_private_data(prepared)
    }

    pub fn prepare_private_data(
        &mut self,
        id: ResourceId,
        namespace: u64,
        key: [u8; 16],
        bytes: &[u8],
    ) -> Result<PreparedPrivateData, ResourceError> {
        let resource = self.resource(id)?;
        let index = resource
            .private_data
            .iter()
            .position(|entry| entry.namespace == namespace && entry.key == key);
        let previous_charge = index.map_or(0, |index| {
            KEY_CHARGE + resource.private_data[index].bytes.len()
        });
        let charge = bytes
            .len()
            .checked_add(KEY_CHARGE)
            .ok_or(ResourceError::ByteCapacity)?;
        let bytes_after = self
            .bytes
            .checked_sub(previous_charge)
            .and_then(|total| total.checked_add(charge))
            .filter(|total| *total <= self.limits.resource_bytes as usize)
            .ok_or(ResourceError::ByteCapacity)?;
        let revision = resource.private_revision;
        let next_revision = revision
            .checked_add(1)
            .ok_or(ResourceError::IdentityExhausted)?;
        let mut owned = Vec::new();
        owned
            .try_reserve_exact(bytes.len())
            .map_err(|_| ResourceError::Allocation)?;
        owned.extend_from_slice(bytes);
        if index.is_none() {
            self.resource_mut(id)?
                .private_data
                .try_reserve(1)
                .map_err(|_| ResourceError::Allocation)?;
        }
        Ok(PreparedPrivateData {
            id,
            namespace,
            key,
            index,
            revision,
            next_revision,
            bytes_before: self.bytes,
            bytes_after,
            previous_charge,
            charge,
            bytes: owned,
        })
    }

    pub fn can_commit_private_data(
        &self,
        prepared: &PreparedPrivateData,
    ) -> Result<(), ResourceError> {
        let resource = self.resource(prepared.id)?;
        if self.bytes != prepared.bytes_before || resource.private_revision != prepared.revision {
            return Err(ResourceError::StaleResource);
        }
        if prepared.index.is_none()
            && resource.private_data.len() == resource.private_data.capacity()
        {
            return Err(ResourceError::StaleResource);
        }
        Ok(())
    }

    pub fn commit_private_data(
        &mut self,
        prepared: PreparedPrivateData,
    ) -> Result<(), ResourceError> {
        self.can_commit_private_data(&prepared)?;
        let resource = self.resource_mut(prepared.id)?;
        if let Some(index) = prepared.index {
            resource.private_data[index].bytes = prepared.bytes;
        } else {
            resource.private_data.push(Entry {
                namespace: prepared.namespace,
                key: prepared.key,
                bytes: prepared.bytes,
            });
        }
        resource.bytes = resource.bytes - prepared.previous_charge + prepared.charge;
        resource.private_revision = prepared.next_revision;
        self.bytes = prepared.bytes_after;
        Ok(())
    }

    pub fn free_private_data(
        &mut self,
        id: ResourceId,
        key: [u8; 16],
    ) -> Result<(), ResourceError> {
        self.free_private_data_for(id, 0, key)
    }

    pub fn free_private_data_for(
        &mut self,
        id: ResourceId,
        namespace: u64,
        key: [u8; 16],
    ) -> Result<(), ResourceError> {
        let resource = self.resource_mut(id)?;
        let index = resource
            .private_data
            .iter()
            .position(|entry| entry.namespace == namespace && entry.key == key)
            .ok_or(ResourceError::NotFound)?;
        let revision = resource
            .private_revision
            .checked_add(1)
            .ok_or(ResourceError::IdentityExhausted)?;
        let bytes = KEY_CHARGE + resource.private_data.remove(index).bytes.len();
        resource.bytes -= bytes;
        resource.private_revision = revision;
        self.bytes -= bytes;
        Ok(())
    }

    pub fn free_private_namespace(
        &mut self,
        id: ResourceId,
        namespace: u64,
    ) -> Result<usize, ResourceError> {
        let resource = self.resource_mut(id)?;
        let bytes: usize = resource
            .private_data
            .iter()
            .filter(|entry| entry.namespace == namespace)
            .map(|entry| KEY_CHARGE + entry.bytes.len())
            .sum();
        if bytes == 0 {
            return Ok(0);
        }
        let revision = resource
            .private_revision
            .checked_add(1)
            .ok_or(ResourceError::IdentityExhausted)?;
        resource
            .private_data
            .retain(|entry| entry.namespace != namespace);
        resource.bytes -= bytes;
        resource.private_revision = revision;
        self.bytes -= bytes;
        Ok(bytes)
    }
}
