use super::{ADDRESS_LIMIT, Access, AddressSpace, CpuSpan, GuestAddress, MemoryError};

impl AddressSpace {
    fn cpu_update<const N: usize>(
        &mut self,
        address: GuestAddress,
        update: impl FnOnce([u8; N]) -> [u8; N],
    ) -> Result<[u8; N], MemoryError> {
        let span = CpuSpan::new(address, N)?;
        let mut old = [0; N];
        self.cpu_read(address, &mut old)?;
        self.cpu_check_span(address, N, Access::Write)?;
        let value = update(old);
        let version = self.advance_version()?;
        self.cpu_copy_in_validated(&span, &value, version);
        Ok(old)
    }

    pub(crate) fn cpu_exchange16(
        &mut self,
        address: GuestAddress,
        value: u16,
    ) -> Result<u16, MemoryError> {
        if 2 <= ADDRESS_LIMIT - u64::from(address.0) {
            return self.exchange16(address, value);
        }
        self.cpu_update(address, |_| value.to_le_bytes())
            .map(u16::from_le_bytes)
    }

    pub(crate) fn cpu_compare_exchange16(
        &mut self,
        address: GuestAddress,
        expected: u16,
        desired: u16,
    ) -> Result<u16, MemoryError> {
        if 2 <= ADDRESS_LIMIT - u64::from(address.0) {
            return self.compare_exchange16(address, expected, desired);
        }
        self.cpu_update(address, |bytes| {
            let old = u16::from_le_bytes(bytes);
            (if old == expected { desired } else { old }).to_le_bytes()
        })
        .map(u16::from_le_bytes)
    }

    pub(crate) fn cpu_exchange_add16(
        &mut self,
        address: GuestAddress,
        source: u16,
    ) -> Result<u16, MemoryError> {
        if 2 <= ADDRESS_LIMIT - u64::from(address.0) {
            return self.exchange_add16(address, source);
        }
        self.cpu_update(address, |bytes| {
            let old = u16::from_le_bytes(bytes);
            old.wrapping_add(source).to_le_bytes()
        })
        .map(u16::from_le_bytes)
    }

    pub(crate) fn cpu_exchange32(
        &mut self,
        address: GuestAddress,
        value: u32,
    ) -> Result<u32, MemoryError> {
        if 4 <= ADDRESS_LIMIT - u64::from(address.0) {
            return self.exchange32(address, value);
        }
        self.cpu_update(address, |_| value.to_le_bytes())
            .map(u32::from_le_bytes)
    }

    pub(crate) fn cpu_compare_exchange32(
        &mut self,
        address: GuestAddress,
        expected: u32,
        desired: u32,
    ) -> Result<u32, MemoryError> {
        if 4 <= ADDRESS_LIMIT - u64::from(address.0) {
            return self.compare_exchange32(address, expected, desired);
        }
        self.cpu_update(address, |bytes| {
            let old = u32::from_le_bytes(bytes);
            (if old == expected { desired } else { old }).to_le_bytes()
        })
        .map(u32::from_le_bytes)
    }

    pub(crate) fn cpu_exchange_add32(
        &mut self,
        address: GuestAddress,
        source: u32,
    ) -> Result<u32, MemoryError> {
        if 4 <= ADDRESS_LIMIT - u64::from(address.0) {
            return self.exchange_add32(address, source);
        }
        self.cpu_update(address, |bytes| {
            let old = u32::from_le_bytes(bytes);
            old.wrapping_add(source).to_le_bytes()
        })
        .map(u32::from_le_bytes)
    }

    pub(crate) fn cpu_bit_test_set32(
        &mut self,
        address: GuestAddress,
        bit: u32,
    ) -> Result<u32, MemoryError> {
        if 4 <= ADDRESS_LIMIT - u64::from(address.0) {
            return self.bit_test_set32(address, bit);
        }
        self.cpu_update(address, |bytes| {
            let old = u32::from_le_bytes(bytes);
            (old | (1_u32 << (bit & 31))).to_le_bytes()
        })
        .map(u32::from_le_bytes)
    }

    pub(crate) fn cpu_bit_test_reset32(
        &mut self,
        address: GuestAddress,
        bit: u32,
    ) -> Result<u32, MemoryError> {
        if 4 <= ADDRESS_LIMIT - u64::from(address.0) {
            return self.bit_test_reset32(address, bit);
        }
        self.cpu_update(address, |bytes| {
            let old = u32::from_le_bytes(bytes);
            (old & !(1_u32 << (bit & 31))).to_le_bytes()
        })
        .map(u32::from_le_bytes)
    }

    pub(crate) fn cpu_atomic_or_u32(
        &mut self,
        address: GuestAddress,
        mask: u32,
    ) -> Result<u32, MemoryError> {
        if 4 <= ADDRESS_LIMIT - u64::from(address.0) {
            return self.atomic_or_u32(address, mask);
        }
        self.cpu_update(address, |bytes| {
            let old = u32::from_le_bytes(bytes);
            (old | mask).to_le_bytes()
        })
        .map(u32::from_le_bytes)
    }

    pub(crate) fn cpu_atomic_and_u32(
        &mut self,
        address: GuestAddress,
        mask: u32,
    ) -> Result<u32, MemoryError> {
        if 4 <= ADDRESS_LIMIT - u64::from(address.0) {
            return self.atomic_and_u32(address, mask);
        }
        self.cpu_update(address, |bytes| {
            let old = u32::from_le_bytes(bytes);
            (old & mask).to_le_bytes()
        })
        .map(u32::from_le_bytes)
    }

    pub(crate) fn cpu_atomic_xor_u32(
        &mut self,
        address: GuestAddress,
        mask: u32,
    ) -> Result<u32, MemoryError> {
        if 4 <= ADDRESS_LIMIT - u64::from(address.0) {
            return self.atomic_xor_u32(address, mask);
        }
        self.cpu_update(address, |bytes| {
            let old = u32::from_le_bytes(bytes);
            (old ^ mask).to_le_bytes()
        })
        .map(u32::from_le_bytes)
    }
}

#[cfg(test)]
#[path = "atomic/tests.rs"]
mod tests;
