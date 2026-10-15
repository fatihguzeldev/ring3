use super::*;

impl AddressSpace {
    pub fn exchange8(&mut self, address: GuestAddress, value: u8) -> Result<u8, MemoryError> {
        let mut old = [0; 1];
        self.read(address, &mut old)?;
        self.check_access(address, old.len(), Access::Write)?;
        let version = self.advance_version()?;
        self.copy_in_validated(address, &[value], version);
        Ok(old[0])
    }

    pub fn exchange16(&mut self, address: GuestAddress, value: u16) -> Result<u16, MemoryError> {
        let mut old = [0; 2];
        self.read(address, &mut old)?;
        self.check_access(address, old.len(), Access::Write)?;
        let version = self.advance_version()?;
        self.copy_in_validated(address, &value.to_le_bytes(), version);
        Ok(u16::from_le_bytes(old))
    }

    pub fn exchange32(&mut self, address: GuestAddress, value: u32) -> Result<u32, MemoryError> {
        let mut old = [0; 4];
        self.read(address, &mut old)?;
        self.check_access(address, old.len(), Access::Write)?;
        let version = self.advance_version()?;
        self.copy_in_validated(address, &value.to_le_bytes(), version);
        Ok(u32::from_le_bytes(old))
    }

    /// returns the old byte and performs the x86 write transaction even on mismatch or identity.
    pub fn compare_exchange8(
        &mut self,
        address: GuestAddress,
        expected: u8,
        desired: u8,
    ) -> Result<u8, MemoryError> {
        let mut bytes = [0; 1];
        self.read(address, &mut bytes)?;
        self.check_access(address, bytes.len(), Access::Write)?;
        let old = bytes[0];
        let value = if old == expected { desired } else { old };
        let version = self.advance_version()?;
        self.copy_in_validated(address, &[value], version);
        Ok(old)
    }

    /// returns the old word and performs the x86 write transaction even on mismatch or identity.
    pub fn compare_exchange16(
        &mut self,
        address: GuestAddress,
        expected: u16,
        desired: u16,
    ) -> Result<u16, MemoryError> {
        let mut bytes = [0; 2];
        self.read(address, &mut bytes)?;
        self.check_access(address, bytes.len(), Access::Write)?;
        let old = u16::from_le_bytes(bytes);
        let value = if old == expected { desired } else { old };
        let version = self.advance_version()?;
        self.copy_in_validated(address, &value.to_le_bytes(), version);
        Ok(old)
    }

    /// returns the old dword and performs the x86 write transaction even on mismatch or identity.
    pub fn compare_exchange32(
        &mut self,
        address: GuestAddress,
        expected: u32,
        desired: u32,
    ) -> Result<u32, MemoryError> {
        let mut bytes = [0; 4];
        self.read(address, &mut bytes)?;
        self.check_access(address, bytes.len(), Access::Write)?;
        let old = u32::from_le_bytes(bytes);
        let value = if old == expected { desired } else { old };
        let version = self.advance_version()?;
        self.copy_in_validated(address, &value.to_le_bytes(), version);
        Ok(old)
    }

    /// returns the old dword and performs the write transaction even when the source is zero.
    pub fn exchange_add32(
        &mut self,
        address: GuestAddress,
        source: u32,
    ) -> Result<u32, MemoryError> {
        let mut bytes = [0; 4];
        self.read(address, &mut bytes)?;
        self.check_access(address, bytes.len(), Access::Write)?;
        let old = u32::from_le_bytes(bytes);
        let value = old.wrapping_add(source);
        let version = self.advance_version()?;
        self.copy_in_validated(address, &value.to_le_bytes(), version);
        Ok(old)
    }

    /// returns the old byte and performs the write transaction even when the source is zero.
    pub fn exchange_add8(&mut self, address: GuestAddress, source: u8) -> Result<u8, MemoryError> {
        let mut bytes = [0; 1];
        self.read(address, &mut bytes)?;
        self.check_access(address, bytes.len(), Access::Write)?;
        let old = bytes[0];
        let value = old.wrapping_add(source);
        let version = self.advance_version()?;
        self.copy_in_validated(address, &[value], version);
        Ok(old)
    }

    /// returns the old word and performs the write transaction even when the source is zero.
    pub fn exchange_add16(
        &mut self,
        address: GuestAddress,
        source: u16,
    ) -> Result<u16, MemoryError> {
        let mut bytes = [0; 2];
        self.read(address, &mut bytes)?;
        self.check_access(address, bytes.len(), Access::Write)?;
        let old = u16::from_le_bytes(bytes);
        let value = old.wrapping_add(source);
        let version = self.advance_version()?;
        self.copy_in_validated(address, &value.to_le_bytes(), version);
        Ok(old)
    }

    /// returns the old dword and performs the write transaction even when the bit is already set.
    pub fn bit_test_set32(&mut self, address: GuestAddress, bit: u32) -> Result<u32, MemoryError> {
        let mut bytes = [0; 4];
        self.read(address, &mut bytes)?;
        self.check_access(address, bytes.len(), Access::Write)?;
        let old = u32::from_le_bytes(bytes);
        let value = old | (1_u32 << (bit & 31));
        let version = self.advance_version()?;
        self.copy_in_validated(address, &value.to_le_bytes(), version);
        Ok(old)
    }

    /// returns the old dword and performs the write transaction even when the bit is already clear.
    pub fn bit_test_reset32(
        &mut self,
        address: GuestAddress,
        bit: u32,
    ) -> Result<u32, MemoryError> {
        let mut bytes = [0; 4];
        self.read(address, &mut bytes)?;
        self.check_access(address, bytes.len(), Access::Write)?;
        let old = u32::from_le_bytes(bytes);
        let value = old & !(1_u32 << (bit & 31));
        let version = self.advance_version()?;
        self.copy_in_validated(address, &value.to_le_bytes(), version);
        Ok(old)
    }

    /// returns the old dword and writes even when the mask is zero or already contained.
    pub fn atomic_or_u32(&mut self, address: GuestAddress, mask: u32) -> Result<u32, MemoryError> {
        let mut bytes = [0; 4];
        self.read(address, &mut bytes)?;
        self.check_access(address, bytes.len(), Access::Write)?;
        let old = u32::from_le_bytes(bytes);
        let value = old | mask;
        let version = self.advance_version()?;
        self.copy_in_validated(address, &value.to_le_bytes(), version);
        Ok(old)
    }

    /// returns the old dword and performs the write transaction even when the mask preserves every bit.
    pub fn atomic_and_u32(&mut self, address: GuestAddress, mask: u32) -> Result<u32, MemoryError> {
        let mut bytes = [0; 4];
        self.read(address, &mut bytes)?;
        self.check_access(address, bytes.len(), Access::Write)?;
        let old = u32::from_le_bytes(bytes);
        let value = old & mask;
        let version = self.advance_version()?;
        self.copy_in_validated(address, &value.to_le_bytes(), version);
        Ok(old)
    }

    /// returns the old dword and performs a write even for a zero mask; restored bytes do not revive code snapshots.
    pub fn atomic_xor_u32(&mut self, address: GuestAddress, mask: u32) -> Result<u32, MemoryError> {
        let mut bytes = [0; 4];
        self.read(address, &mut bytes)?;
        self.check_access(address, bytes.len(), Access::Write)?;
        let old = u32::from_le_bytes(bytes);
        let value = old ^ mask;
        let version = self.advance_version()?;
        self.copy_in_validated(address, &value.to_le_bytes(), version);
        Ok(old)
    }
}

#[cfg(test)]
mod tests;
