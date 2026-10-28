use super::{MemoryError, PAGE_SIZE, filled};
use std::ops::{Deref, DerefMut};

pub(super) struct Backing {
    storage: Box<[u8]>,
    offset: usize,
    length: usize,
    #[cfg(feature = "external-cpu")]
    external: Option<crate::abi::external_memory::ExternalMemory>,
}

impl Backing {
    pub(super) fn zeroed(length: usize) -> Result<Self, MemoryError> {
        let mask = PAGE_SIZE as usize - 1;
        let allocation = length
            .checked_add(mask)
            .filter(|&allocation| length != 0 && allocation <= isize::MAX as usize)
            .ok_or(MemoryError::Capacity)?;
        let storage = filled(allocation, 0)?.into_boxed_slice();
        let offset = (PAGE_SIZE as usize - (storage.as_ptr() as usize & mask)) & mask;
        Ok(Self {
            storage,
            offset,
            length,
            #[cfg(feature = "external-cpu")]
            external: None,
        })
    }

    #[cfg(feature = "external-cpu")]
    pub(super) fn attach_external(
        &mut self,
        mut external: crate::abi::external_memory::ExternalMemory,
    ) -> Result<(), MemoryError> {
        if self.external.is_some() || external.len() != self.length {
            return Err(MemoryError::InvalidRange);
        }
        external.copy_from_slice(self);
        self.external = Some(external);
        self.storage = Box::new([]);
        self.offset = 0;
        Ok(())
    }

    #[cfg(feature = "external-cpu")]
    pub(super) fn release_external(&mut self) -> Result<(), MemoryError> {
        if self.external.is_none() {
            return Err(MemoryError::InvalidRange);
        }
        let mut owned = Self::zeroed(self.length)?;
        owned.copy_from_slice(self);
        *self = owned;
        Ok(())
    }
}

impl Deref for Backing {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        #[cfg(feature = "external-cpu")]
        if let Some(external) = self.external.as_ref() {
            return external;
        }
        &self.storage[self.offset..self.offset + self.length]
    }
}

impl DerefMut for Backing {
    fn deref_mut(&mut self) -> &mut Self::Target {
        #[cfg(feature = "external-cpu")]
        if let Some(external) = self.external.as_mut() {
            return external;
        }
        &mut self.storage[self.offset..self.offset + self.length]
    }
}

impl AsRef<[u8]> for Backing {
    fn as_ref(&self) -> &[u8] {
        self
    }
}

#[cfg(test)]
impl Clone for Backing {
    fn clone(&self) -> Self {
        let mut copy = Self::zeroed(self.length).expect("test backing clone allocation");
        copy.copy_from_slice(self);
        copy
    }
}

#[cfg(test)]
impl std::fmt::Debug for Backing {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.deref().fmt(formatter)
    }
}

#[cfg(test)]
impl PartialEq for Backing {
    fn eq(&self, other: &Self) -> bool {
        self.deref() == other.deref()
    }
}

#[cfg(test)]
impl Eq for Backing {}

#[cfg(test)]
mod tests;
