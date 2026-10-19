use super::{MemoryError, PAGE_SIZE, filled};
use std::ops::{Deref, DerefMut};

pub(super) struct Backing {
    storage: Box<[u8]>,
    offset: usize,
    length: usize,
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
        })
    }
}

impl Deref for Backing {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        &self.storage[self.offset..self.offset + self.length]
    }
}

impl DerefMut for Backing {
    fn deref_mut(&mut self) -> &mut Self::Target {
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
