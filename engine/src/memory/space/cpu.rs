//! CPU linear-bus spans wrap after segment authority has checked the logical span.
//! ordinary public AddressSpace accesses retain their strict contiguous contract.

mod atomic;
mod read;
mod write;

use super::{ADDRESS_LIMIT, Access, AddressSpace, GuestAddress, MemoryError};

#[derive(Clone, Copy)]
pub(super) struct CpuPart {
    pub(super) address: GuestAddress,
    pub(super) offset: usize,
    pub(super) length: usize,
}

/// at most two ordered, nonwrapping pieces of one CPU linear span.
pub(super) struct CpuSpan {
    parts: [CpuPart; 2],
    count: usize,
}

impl CpuSpan {
    pub(super) fn new(address: GuestAddress, length: usize) -> Result<Self, MemoryError> {
        if length as u64 > ADDRESS_LIMIT {
            return Err(MemoryError::InvalidRange);
        }
        let first = (length as u64).min(ADDRESS_LIMIT - u64::from(address.0)) as usize;
        let second = length - first;
        Ok(Self {
            parts: [
                CpuPart {
                    address,
                    offset: 0,
                    length: first,
                },
                CpuPart {
                    address: GuestAddress(0),
                    offset: first,
                    length: second,
                },
            ],
            count: if second == 0 { 1 } else { 2 },
        })
    }

    pub(super) fn parts(&self) -> impl Iterator<Item = CpuPart> + '_ {
        self.parts[..self.count].iter().copied()
    }
}

impl AddressSpace {
    pub(crate) fn cpu_check_span(
        &self,
        address: GuestAddress,
        length: usize,
        access: Access,
    ) -> Result<(), MemoryError> {
        if length as u64 <= ADDRESS_LIMIT - u64::from(address.0) {
            return self.check_span(address, length, access);
        }
        for part in CpuSpan::new(address, length)?.parts() {
            self.check_span(part.address, part.length, access)?;
        }
        Ok(())
    }
}

impl AddressSpace {
    pub(crate) fn cpu_check_masked(
        &self,
        address: GuestAddress,
        length: usize,
        mask: u32,
        access: Access,
    ) -> Result<(), MemoryError> {
        if length == 0 || length > 32 || (length < 32 && mask >> length != 0) {
            return Err(MemoryError::InvalidRange);
        }
        if length as u64 <= ADDRESS_LIMIT - u64::from(address.0) {
            return self.check_masked(address, length, mask, access);
        }
        for index in 0..length {
            if mask & (1 << index) != 0 {
                self.check_access(
                    GuestAddress(address.0.wrapping_add(index as u32)),
                    1,
                    access,
                )?;
            }
        }
        Ok(())
    }
}
