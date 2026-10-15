use super::{ResourceError, ResourceStore};
use crate::graphics::ResourceId;

impl ResourceStore {
    pub fn private_data(&self, id: ResourceId, key: [u8; 16]) -> Result<&[u8], ResourceError> {
        self.resource(id)?
            .private_data
            .iter()
            .find(|(stored, _)| *stored == key)
            .map(|(_, bytes)| bytes.as_slice())
            .ok_or(ResourceError::NotFound)
    }

    pub fn set_private_data(
        &mut self,
        id: ResourceId,
        key: [u8; 16],
        bytes: &[u8],
    ) -> Result<(), ResourceError> {
        let resource = self.resource(id)?;
        let position = resource
            .private_data
            .iter()
            .position(|(stored, _)| *stored == key);
        let previous = position.map_or(0, |index| 16 + resource.private_data[index].1.len());
        let charge = bytes
            .len()
            .checked_add(16)
            .ok_or(ResourceError::ByteCapacity)?;
        let total = self
            .bytes
            .checked_sub(previous)
            .and_then(|total| total.checked_add(charge))
            .filter(|total| *total <= self.limits.resource_bytes as usize)
            .ok_or(ResourceError::ByteCapacity)?;
        let mut owned = Vec::new();
        owned
            .try_reserve_exact(bytes.len())
            .map_err(|_| ResourceError::Allocation)?;
        owned.extend_from_slice(bytes);
        let resource = self.resource_mut(id)?;
        if let Some(index) = position {
            resource.private_data[index].1 = owned;
        } else {
            resource
                .private_data
                .try_reserve(1)
                .map_err(|_| ResourceError::Allocation)?;
            resource.private_data.push((key, owned));
        }
        resource.bytes = resource.bytes - previous + charge;
        self.bytes = total;
        Ok(())
    }

    pub fn free_private_data(
        &mut self,
        id: ResourceId,
        key: [u8; 16],
    ) -> Result<(), ResourceError> {
        let resource = self.resource_mut(id)?;
        let index = resource
            .private_data
            .iter()
            .position(|(stored, _)| *stored == key)
            .ok_or(ResourceError::NotFound)?;
        let bytes = 16 + resource.private_data.remove(index).1.len();
        resource.bytes -= bytes;
        self.bytes -= bytes;
        Ok(())
    }
}
