use crate::memory::{
    AddressSpace, GuestAddress, MemoryError, PAGE_SIZE, PageInitialization, PageRange, Permissions,
};
#[derive(Clone, Copy)]
pub(in crate::process::windows) struct GuestCallbackScratch32 {
    pub(in crate::process::windows) address: GuestAddress,
    range: PageRange,
    identity: u64,
    mapping: u64,
}
impl GuestCallbackScratch32 {
    pub(in crate::process::windows) fn range(self) -> PageRange {
        self.range
    }
    pub(in crate::process::windows) fn allocate(
        memory: &mut AddressSpace,
        bounds: PageRange,
        bytes: &[u8],
    ) -> Result<Self, MemoryError> {
        if bytes.len() > PAGE_SIZE as usize {
            return Err(MemoryError::InvalidRange);
        }
        Self::allocate_span(memory, bounds, bytes)
    }
    pub(in crate::process::windows) fn allocate_span(
        memory: &mut AddressSpace,
        bounds: PageRange,
        bytes: &[u8],
    ) -> Result<Self, MemoryError> {
        if bounds.first == 0 {
            return Err(MemoryError::InvalidRange);
        }
        let pages = u32::try_from(bytes.len().max(1).div_ceil(PAGE_SIZE as usize))
            .map_err(|_| MemoryError::InvalidRange)?;
        let range = memory.find_free_range(bounds, pages, PAGE_SIZE)?;
        let address = GuestAddress(range.first as u32 * PAGE_SIZE);
        if pages == 1 {
            memory.install_pages_with_word_writes(
                &[PageInitialization {
                    address,
                    permissions: Permissions::READ_WRITE,
                    bytes,
                }],
                &[],
            )?;
        } else {
            let mut initializers = Vec::new();
            initializers
                .try_reserve_exact(pages as usize)
                .map_err(|_| MemoryError::Allocation)?;
            for (index, bytes) in bytes.chunks(PAGE_SIZE as usize).enumerate() {
                initializers.push(PageInitialization {
                    address: GuestAddress(address.0 + index as u32 * PAGE_SIZE),
                    permissions: Permissions::READ_WRITE,
                    bytes,
                });
            }
            memory.install_pages_with_word_writes(&initializers, &[])?;
        }
        Ok(Self {
            address,
            range,
            identity: memory.identity(),
            mapping: memory
                .mapping_version_at(address)
                .expect("installed callback scratch"),
        })
    }
    pub(in crate::process::windows) fn check(
        self,
        memory: &AddressSpace,
    ) -> Result<(), MemoryError> {
        if memory.identity() != self.identity {
            return Err(MemoryError::InvalidRange);
        }
        for page in self.range.first..self.range.first + self.range.count {
            let address = GuestAddress(page as u32 * PAGE_SIZE);
            if memory.mapping_version_at(address)? != self.mapping
                || memory.permissions_at(address)? != Permissions::READ_WRITE
            {
                return Err(MemoryError::InvalidRange);
            }
        }
        Ok(())
    }
    pub(in crate::process::windows) fn release(
        self,
        memory: &mut AddressSpace,
    ) -> Result<(), MemoryError> {
        self.check(memory)?;
        memory.unmap_ranges_with_word_writes(&[self.range], &[])?;
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn nested_loans_keep_identity_and_stale_release_cannot_unmap_replacement() {
        let mut memory = AddressSpace::new(8).unwrap();
        let bounds = PageRange::new(GuestAddress(0x10000), 4).unwrap();
        let first = GuestCallbackScratch32::allocate(&mut memory, bounds, b"first\0").unwrap();
        let second = GuestCallbackScratch32::allocate(&mut memory, bounds, b"second\0").unwrap();
        assert_ne!(first.address, second.address);
        first.release(&mut memory).unwrap();
        let replacement =
            GuestCallbackScratch32::allocate(&mut memory, bounds, b"replacement\0").unwrap();
        assert_eq!(replacement.address, first.address);
        assert_eq!(first.release(&mut memory), Err(MemoryError::InvalidRange));
        replacement.check(&memory).unwrap();
        second.check(&memory).unwrap();
        second.release(&mut memory).unwrap();
        replacement.release(&mut memory).unwrap();
    }
    #[test]
    fn oversized_payload_does_not_publish_a_page() {
        let mut memory = AddressSpace::new(8).unwrap();
        let bounds = PageRange::new(GuestAddress(0x10000), 4).unwrap();
        let before = memory.transaction_version();
        assert!(matches!(
            GuestCallbackScratch32::allocate(&mut memory, bounds, &[0; 4097]),
            Err(MemoryError::InvalidRange)
        ));
        assert_eq!(memory.transaction_version(), before);
        assert!(matches!(
            memory.permissions_at(GuestAddress(0x10000)),
            Err(MemoryError::NotMapped { .. })
        ));
    }
    #[test]
    fn multipage_loan_validates_every_page_before_release() {
        let mut memory = AddressSpace::new(8).unwrap();
        let bounds = PageRange::new(GuestAddress(0x10000), 4).unwrap();
        let loan = GuestCallbackScratch32::allocate_span(&mut memory, bounds, &[7; 5000]).unwrap();
        assert_eq!(loan.range().count, 2);
        let tail = PageRange::new(GuestAddress(loan.address.0 + PAGE_SIZE), 1).unwrap();
        memory.unmap_ranges_with_word_writes(&[tail], &[]).unwrap();
        memory.map_zeroed(tail, Permissions::READ_WRITE).unwrap();
        let before = memory.transaction_version();
        assert_eq!(loan.release(&mut memory), Err(MemoryError::InvalidRange));
        assert_eq!(memory.transaction_version(), before);
        assert_eq!(
            memory.permissions_at(loan.address),
            Ok(Permissions::READ_WRITE)
        );
    }
}
