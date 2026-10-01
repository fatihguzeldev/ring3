use super::address::{ADDRESS_LIMIT, GUEST_PAGES};
use super::*;

pub struct AddressSpace {
    backing: Box<[u8]>,
    mappings: Box<[u32]>,
    permissions: Box<[Permissions]>,
    free: Vec<u32>,
}

impl AddressSpace {
    pub fn new(resident_pages: u32) -> Result<Self, MemoryError> {
        let bytes = u64::from(resident_pages) * u64::from(PAGE_SIZE);
        if resident_pages == 0 || bytes > u64::from(u32::MAX) || bytes > isize::MAX as u64 {
            return Err(MemoryError::Capacity);
        }
        let backing = filled(bytes as usize, 0)?.into_boxed_slice();
        let mappings = filled(GUEST_PAGES, 0)?.into_boxed_slice();
        let permissions = filled(resident_pages as usize, Permissions::NONE)?.into_boxed_slice();
        let mut free = Vec::new();
        free.try_reserve_exact(resident_pages as usize)
            .map_err(|_| MemoryError::Allocation)?;
        free.extend((0..resident_pages).rev());
        Ok(Self {
            backing,
            mappings,
            permissions,
            free,
        })
    }

    pub fn capacity_pages(&self) -> u32 {
        self.permissions.len() as u32
    }

    pub fn mapped_pages(&self) -> u32 {
        self.capacity_pages() - self.free.len() as u32
    }

    pub fn map_zeroed(
        &mut self,
        range: PageRange,
        permissions: Permissions,
    ) -> Result<(), MemoryError> {
        for page in range.indices() {
            if self.mappings[page] != 0 {
                return Err(MemoryError::AlreadyMapped {
                    address: page_address(page),
                });
            }
        }
        if range.count > self.free.len() {
            return Err(MemoryError::Capacity);
        }
        for page in range.indices() {
            let slot = self.free.pop().unwrap();
            let offset = slot as usize * PAGE_SIZE as usize;
            self.backing[offset..offset + PAGE_SIZE as usize].fill(0);
            self.permissions[slot as usize] = permissions;
            self.mappings[page] = slot + 1;
        }
        Ok(())
    }

    pub fn protect(
        &mut self,
        range: PageRange,
        permissions: Permissions,
    ) -> Result<(), MemoryError> {
        self.require_mapped(range)?;
        for page in range.indices() {
            let slot = self.mappings[page] as usize - 1;
            self.permissions[slot] = permissions;
        }
        Ok(())
    }

    pub fn unmap(&mut self, range: PageRange) -> Result<(), MemoryError> {
        self.require_mapped(range)?;
        for page in range.indices() {
            let slot = self.mappings[page] - 1;
            self.mappings[page] = 0;
            self.permissions[slot as usize] = Permissions::NONE;
            self.free.push(slot);
        }
        Ok(())
    }

    pub fn resolve(
        &self,
        address: GuestAddress,
        access: Access,
    ) -> Result<BackingOffset, MemoryError> {
        let page = (address.0 / PAGE_SIZE) as usize;
        let slot = self.access_slot(page, address, access)?;
        Ok(BackingOffset(
            slot as u32 * PAGE_SIZE + address.0 % PAGE_SIZE,
        ))
    }

    pub fn read(&self, address: GuestAddress, output: &mut [u8]) -> Result<(), MemoryError> {
        self.copy_out(address, output, Access::Read)
    }

    pub fn fetch(&self, address: GuestAddress, output: &mut [u8]) -> Result<(), MemoryError> {
        self.copy_out(address, output, Access::Execute)
    }

    pub fn write(&mut self, address: GuestAddress, input: &[u8]) -> Result<(), MemoryError> {
        self.check_access(address, input.len(), Access::Write)?;
        let mut copied = 0;
        while copied < input.len() {
            let current = address.0 as u64 + copied as u64;
            let page = current as usize / PAGE_SIZE as usize;
            let within = current as usize % PAGE_SIZE as usize;
            let count = (PAGE_SIZE as usize - within).min(input.len() - copied);
            let offset = (self.mappings[page] as usize - 1) * PAGE_SIZE as usize + within;
            self.backing[offset..offset + count].copy_from_slice(&input[copied..copied + count]);
            copied += count;
        }
        Ok(())
    }

    fn copy_out(
        &self,
        address: GuestAddress,
        output: &mut [u8],
        access: Access,
    ) -> Result<(), MemoryError> {
        self.check_access(address, output.len(), access)?;
        let mut copied = 0;
        while copied < output.len() {
            let current = address.0 as u64 + copied as u64;
            let page = current as usize / PAGE_SIZE as usize;
            let within = current as usize % PAGE_SIZE as usize;
            let count = (PAGE_SIZE as usize - within).min(output.len() - copied);
            let offset = (self.mappings[page] as usize - 1) * PAGE_SIZE as usize + within;
            output[copied..copied + count].copy_from_slice(&self.backing[offset..offset + count]);
            copied += count;
        }
        Ok(())
    }

    fn check_access(
        &self,
        address: GuestAddress,
        length: usize,
        access: Access,
    ) -> Result<(), MemoryError> {
        if length == 0 {
            return Ok(());
        }
        let end = u64::from(address.0).checked_add(length as u64);
        let Some(end) = end.filter(|&end| end <= ADDRESS_LIMIT) else {
            return Err(fault(address, access, FaultReason::AddressOverflow));
        };
        let first = address.0 as usize / PAGE_SIZE as usize;
        let last = ((end - 1) / u64::from(PAGE_SIZE)) as usize;
        for page in first..=last {
            let at = GuestAddress(address.0.max(page_address(page).0));
            self.access_slot(page, at, access)?;
        }
        Ok(())
    }

    fn access_slot(
        &self,
        page: usize,
        address: GuestAddress,
        access: Access,
    ) -> Result<usize, MemoryError> {
        let mapping = self.mappings[page];
        if mapping == 0 {
            return Err(fault(address, access, FaultReason::Unmapped));
        }
        let slot = mapping as usize - 1;
        if !self.permissions[slot].allows(access) {
            return Err(fault(address, access, FaultReason::Permission));
        }
        Ok(slot)
    }

    fn require_mapped(&self, range: PageRange) -> Result<(), MemoryError> {
        for page in range.indices() {
            if self.mappings[page] == 0 {
                return Err(MemoryError::NotMapped {
                    address: page_address(page),
                });
            }
        }
        Ok(())
    }
}

fn filled<T: Clone>(count: usize, value: T) -> Result<Vec<T>, MemoryError> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(count)
        .map_err(|_| MemoryError::Allocation)?;
    values.resize(count, value);
    Ok(values)
}

fn page_address(page: usize) -> GuestAddress {
    GuestAddress(page as u32 * PAGE_SIZE)
}

fn fault(address: GuestAddress, access: Access, reason: FaultReason) -> MemoryError {
    MemoryError::Fault(MemoryFault {
        address,
        access,
        reason,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocates_only_the_requested_resident_capacity() {
        let space = AddressSpace::new(2).unwrap();
        assert_eq!(space.capacity_pages(), 2);
        assert_eq!(space.mapped_pages(), 0);
    }

    #[test]
    fn rejects_unrepresentable_capacity_before_allocating() {
        assert!(matches!(AddressSpace::new(0), Err(MemoryError::Capacity)));
        assert!(matches!(
            AddressSpace::new(u32::MAX),
            Err(MemoryError::Capacity)
        ));
    }
}
