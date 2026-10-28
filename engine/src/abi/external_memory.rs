#![allow(unsafe_code)]

use std::ops::{Deref, DerefMut};

pub(crate) struct ExternalMemory {
    address: usize,
    length: usize,
}

impl ExternalMemory {
    pub(crate) fn is_cpu_page_tables_for(&self, backing: usize) -> bool {
        cpu_page_tables_range(self.address, self.length, backing)
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn admit(address: u32, length: u32) -> Option<Self> {
        unsafe extern "C" {
            static __heap_base: u8;
        }
        let heap = unsafe { &raw const __heap_base } as usize;
        let available = core::arch::wasm32::memory_size(0).checked_mul(65536)?;
        let end = (address as usize).checked_add(length as usize)?;
        if heap < 512 * 1024 * 1024
            || !valid_range(address as usize, length as usize, available)
            || end > 511 * 1024 * 1024
        {
            return None;
        }
        Some(Self {
            address: address as usize,
            length: length as usize,
        })
    }
}

fn cpu_page_tables_range(address: usize, length: usize, backing: usize) -> bool {
    const PREFIX: usize = 16 * 1024 * 1024;
    const DIRECTORY: usize = 0x200000;
    const TABLE_BYTES: usize = 4096 * 1025;
    let Some(ram) = backing.checked_sub(PREFIX) else {
        return false;
    };
    length == TABLE_BYTES
        && ram.checked_add(DIRECTORY) == Some(address)
        && address
            .checked_add(length)
            .is_some_and(|end| end <= backing)
}

fn valid_range(address: usize, length: usize, available: usize) -> bool {
    address != 0
        && address.is_multiple_of(4096)
        && length != 0
        && length.is_multiple_of(4096)
        && length <= isize::MAX as usize
        && address
            .checked_add(length)
            .is_some_and(|end| end <= available)
}

impl Deref for ExternalMemory {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        // the external cpu owns this reserved low-memory allocation until release.
        unsafe { std::slice::from_raw_parts(self.address as *const u8, self.length) }
    }
}

impl DerefMut for ExternalMemory {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // host cpu slices and engine calls are synchronous and mutually exclusive.
        unsafe { std::slice::from_raw_parts_mut(self.address as *mut u8, self.length) }
    }
}

#[cfg(test)]
mod tests {
    use super::{cpu_page_tables_range, valid_range};

    #[test]
    fn external_dirty_collect_range_is_exact_and_disjoint_from_backing() {
        let ram = 80 * 1024 * 1024;
        let backing = ram + 16 * 1024 * 1024;
        let address = ram + 0x200000;
        let length = 4096 * 1025;
        assert!(cpu_page_tables_range(address, length, backing));
        assert!(!cpu_page_tables_range(address + 4096, length, backing));
        assert!(!cpu_page_tables_range(address, length - 4096, backing));
        assert!(!cpu_page_tables_range(backing, length, backing));
        assert!(!cpu_page_tables_range(ram + 0x100000, length, backing));
        assert!(!cpu_page_tables_range(address, length, 4096));
        assert!(!cpu_page_tables_range(
            usize::MAX - 4095,
            length,
            usize::MAX
        ));
    }

    #[test]
    fn external_range_rejects_alignment_overflow_and_out_of_memory() {
        assert!(valid_range(4096, 8192, 12288));
        for (address, length, available) in [
            (0, 4096, 8192),
            (1, 4096, 8192),
            (4096, 0, 8192),
            (4096, 4095, 8192),
            (4096, 8192, 8192),
            (usize::MAX - 4095, 4096, usize::MAX),
        ] {
            assert!(!valid_range(address, length, available));
        }
    }
}
