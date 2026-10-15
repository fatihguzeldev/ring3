use super::*;

impl AddressSpace {
    pub(crate) fn fetch_code_wrapping(
        &self,
        address: GuestAddress,
        output: &mut [u8],
    ) -> Result<(), MemoryError> {
        let split = code_split(address, output.len())?;
        if split == output.len() {
            return self.fetch(address, output);
        }
        // both linear ranges are executable before publishing any fetched bytes.
        self.check_access(address, split, Access::Execute)?;
        self.check_access(GuestAddress(0), output.len() - split, Access::Execute)?;
        let (first, second) = output.split_at_mut(split);
        self.fetch(address, first)?;
        self.fetch(GuestAddress(0), second)
    }

    pub(crate) fn snapshot_code_wrapping(
        &self,
        address: GuestAddress,
        length: usize,
    ) -> Result<CodeSnapshot, MemoryError> {
        if length == 0 {
            return Err(MemoryError::InvalidRange);
        }
        let split = code_split(address, length)?;
        if split == length {
            return self.snapshot_code(address, length);
        }
        self.check_access(address, split, Access::Execute)?;
        self.check_access(GuestAddress(0), length - split, Access::Execute)?;
        let first = (address.0 / PAGE_SIZE) as usize;
        let count = (u64::from(address.0 % PAGE_SIZE) + length as u64)
            .div_ceil(u64::from(PAGE_SIZE)) as usize;
        let mut versions = Vec::new();
        versions
            .try_reserve_exact(count)
            .map_err(|_| MemoryError::Allocation)?;
        for index in 0..count {
            let page = (first + index) % GUEST_PAGES;
            versions.push(self.page_version(page));
        }
        Ok(CodeSnapshot {
            identity: self.identity,
            first,
            versions: versions.into_boxed_slice(),
        })
    }
}

fn code_split(address: GuestAddress, length: usize) -> Result<usize, MemoryError> {
    if length as u64 > ADDRESS_LIMIT {
        return Err(MemoryError::InvalidRange);
    }
    Ok((ADDRESS_LIMIT - u64::from(address.0)).min(length as u64) as usize)
}

#[cfg(test)]
mod tests;
