#![expect(
    clippy::missing_errors_doc,
    reason = "all operations return explicit range, mapping, or permission errors"
)]

use std::collections::BTreeMap;

pub const PAGE_SIZE: u64 = 4096;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Access {
    Read,
    Write,
    Execute,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Permissions {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
}

impl Permissions {
    pub const NONE: Self = Self {
        read: false,
        write: false,
        execute: false,
    };
    pub const READ: Self = Self {
        read: true,
        ..Self::NONE
    };
    pub const READ_WRITE: Self = Self {
        write: true,
        ..Self::READ
    };
    pub const READ_EXECUTE: Self = Self {
        execute: true,
        ..Self::READ
    };

    fn permits(self, access: Access) -> bool {
        match access {
            Access::Read => self.read,
            Access::Write => self.write,
            Access::Execute => self.execute,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MemoryError {
    UnalignedRange,
    AddressOverflow,
    PageLimitExceeded,
    AlreadyMapped { address: u64 },
    Unmapped { address: u64 },
    PermissionDenied { address: u64, access: Access },
}

struct Page {
    bytes: Box<[u8; 4096]>,
    permissions: Permissions,
}

pub struct GuestMemory {
    pages: BTreeMap<u64, Page>,
    page_limit: u32,
}

impl GuestMemory {
    #[must_use]
    pub fn new(page_limit: u32) -> Self {
        Self {
            pages: BTreeMap::new(),
            page_limit,
        }
    }

    #[must_use]
    pub fn mapped_pages(&self) -> usize {
        self.pages.len()
    }

    pub fn map_zeroed(
        &mut self,
        address: u64,
        length: u64,
        permissions: Permissions,
    ) -> Result<(), MemoryError> {
        let range = page_range(address, length)?;
        if length / PAGE_SIZE + self.pages.len() as u64 > u64::from(self.page_limit) {
            return Err(MemoryError::PageLimitExceeded);
        }
        for index in range.clone() {
            if self.pages.contains_key(&index) {
                return Err(MemoryError::AlreadyMapped {
                    address: index * PAGE_SIZE,
                });
            }
        }
        for index in range {
            self.pages.insert(
                index,
                Page {
                    bytes: Box::new([0; 4096]),
                    permissions,
                },
            );
        }
        Ok(())
    }

    pub fn protect(
        &mut self,
        address: u64,
        length: u64,
        permissions: Permissions,
    ) -> Result<(), MemoryError> {
        let range = page_range(address, length)?;
        for index in range.clone() {
            if !self.pages.contains_key(&index) {
                return Err(MemoryError::Unmapped {
                    address: index * PAGE_SIZE,
                });
            }
        }
        for (_, page) in self.pages.range_mut(range) {
            page.permissions = permissions;
        }
        Ok(())
    }

    pub fn read(&self, address: u64, output: &mut [u8]) -> Result<(), MemoryError> {
        self.copy_out(address, output, Access::Read)
    }

    pub fn fetch(&self, address: u64, output: &mut [u8]) -> Result<(), MemoryError> {
        self.copy_out(address, output, Access::Execute)
    }

    pub fn write(&mut self, mut address: u64, mut input: &[u8]) -> Result<(), MemoryError> {
        self.check_access(address, input.len(), Access::Write)?;
        while !input.is_empty() {
            let (index, offset, count) = chunk(address, input.len());
            let page = self
                .pages
                .get_mut(&index)
                .ok_or(MemoryError::Unmapped { address })?;
            page.bytes[offset..offset + count].copy_from_slice(&input[..count]);
            address += count as u64;
            input = &input[count..];
        }
        Ok(())
    }

    fn copy_out(
        &self,
        mut address: u64,
        mut output: &mut [u8],
        access: Access,
    ) -> Result<(), MemoryError> {
        self.check_access(address, output.len(), access)?;
        while !output.is_empty() {
            let (index, offset, count) = chunk(address, output.len());
            output[..count].copy_from_slice(&self.pages[&index].bytes[offset..offset + count]);
            address += count as u64;
            output = &mut output[count..];
        }
        Ok(())
    }

    pub(super) fn check_access(
        &self,
        address: u64,
        length: usize,
        access: Access,
    ) -> Result<(), MemoryError> {
        let end = address
            .checked_add(length as u64)
            .ok_or(MemoryError::AddressOverflow)?;
        let mut cursor = address;
        while cursor < end {
            let index = cursor / PAGE_SIZE;
            let page = self
                .pages
                .get(&index)
                .ok_or(MemoryError::Unmapped { address: cursor })?;
            if !page.permissions.permits(access) {
                return Err(MemoryError::PermissionDenied {
                    address: cursor,
                    access,
                });
            }
            let count = (PAGE_SIZE - cursor % PAGE_SIZE).min(end - cursor);
            cursor += count;
        }
        Ok(())
    }
}

fn page_range(address: u64, length: u64) -> Result<std::ops::Range<u64>, MemoryError> {
    if !address.is_multiple_of(PAGE_SIZE) || length == 0 || !length.is_multiple_of(PAGE_SIZE) {
        return Err(MemoryError::UnalignedRange);
    }
    let end = address
        .checked_add(length)
        .ok_or(MemoryError::AddressOverflow)?;
    Ok(address / PAGE_SIZE..end / PAGE_SIZE)
}

fn chunk(address: u64, remaining: usize) -> (u64, usize, usize) {
    let offset = usize::try_from(address % PAGE_SIZE).expect("page offset fits usize");
    (address / PAGE_SIZE, offset, remaining.min(4096 - offset))
}
