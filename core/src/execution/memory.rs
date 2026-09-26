#![expect(
    clippy::missing_errors_doc,
    reason = "all operations return explicit range, mapping, or permission errors"
)]

use std::collections::{BTreeSet, HashMap};

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
    pages: HashMap<u64, Page>,
    occupied: BTreeSet<u64>,
    page_limit: u32,
}

impl GuestMemory {
    #[must_use]
    pub fn new(page_limit: u32) -> Self {
        Self {
            pages: HashMap::new(),
            occupied: BTreeSet::new(),
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
            self.occupied.insert(index);
        }
        Ok(())
    }

    pub(super) fn unmap(&mut self, address: u64, length: u64) -> Result<(), MemoryError> {
        let range = page_range(address, length)?;
        for index in range.clone() {
            if !self.pages.contains_key(&index) {
                return Err(MemoryError::Unmapped {
                    address: index * PAGE_SIZE,
                });
            }
        }
        for index in range {
            self.pages.remove(&index);
            self.occupied.remove(&index);
        }
        Ok(())
    }

    pub(super) fn first_free_span(
        &self,
        start: u64,
        end: u64,
        length: u64,
    ) -> Result<Option<u64>, MemoryError> {
        let window = page_range(
            start,
            end.checked_sub(start).ok_or(MemoryError::AddressOverflow)?,
        )?;
        let count = page_range(0, length)?.end;
        let mut cursor = window.start;
        for &index in self.occupied.range(window.clone()) {
            if index - cursor >= count {
                return Ok(Some(cursor * PAGE_SIZE));
            }
            cursor = index + 1;
        }
        Ok((window.end - cursor >= count).then_some(cursor * PAGE_SIZE))
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
        for index in range {
            let page = self.pages.get_mut(&index).ok_or(MemoryError::Unmapped {
                address: index * PAGE_SIZE,
            })?;
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

    pub(super) fn fill(
        &mut self,
        mut address: u64,
        mut length: usize,
        value: u8,
    ) -> Result<(), MemoryError> {
        self.check_access(address, length, Access::Write)?;
        while length != 0 {
            let (index, offset, count) = chunk(address, length);
            let page = self.pages.get_mut(&index).expect("fill range was checked");
            page.bytes[offset..offset + count].fill(value);
            address += count as u64;
            length -= count;
        }
        Ok(())
    }

    fn copy_out(
        &self,
        mut address: u64,
        mut output: &mut [u8],
        access: Access,
    ) -> Result<(), MemoryError> {
        let (index, offset, count) = chunk(address, output.len());
        if count == output.len() && count != 0 {
            address
                .checked_add(count as u64)
                .ok_or(MemoryError::AddressOverflow)?;
            let page = self
                .pages
                .get(&index)
                .ok_or(MemoryError::Unmapped { address })?;
            if !page.permissions.permits(access) {
                return Err(MemoryError::PermissionDenied { address, access });
            }
            output.copy_from_slice(&page.bytes[offset..offset + count]);
            return Ok(());
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sparse_free_spans_respect_holes_window_edges_and_alignment() {
        let mut memory = GuestMemory::new(8);
        memory
            .map_zeroed(0x2000, PAGE_SIZE, Permissions::NONE)
            .unwrap();
        memory
            .map_zeroed(0x5000, PAGE_SIZE, Permissions::NONE)
            .unwrap();
        assert_eq!(
            memory.first_free_span(0x1000, 0x7000, PAGE_SIZE),
            Ok(Some(0x1000))
        );
        assert_eq!(
            memory.first_free_span(0x1000, 0x7000, PAGE_SIZE * 2),
            Ok(Some(0x3000))
        );
        assert_eq!(
            memory.first_free_span(0x5000, 0x7000, PAGE_SIZE),
            Ok(Some(0x6000))
        );
        assert_eq!(
            memory.first_free_span(0x1000, 0x7000, PAGE_SIZE * 3),
            Ok(None)
        );
        assert_eq!(
            memory.first_free_span(0x1000, 0x2000, PAGE_SIZE * 2),
            Ok(None)
        );
        for (start, end, length) in [
            (1, 0x2000, PAGE_SIZE),
            (0, 0x2001, PAGE_SIZE),
            (0, 0x2000, 0),
            (0, 0x2000, 1),
        ] {
            assert_eq!(
                memory.first_free_span(start, end, length),
                Err(MemoryError::UnalignedRange)
            );
        }
        assert_eq!(
            memory.first_free_span(0x2000, 0x1000, PAGE_SIZE),
            Err(MemoryError::AddressOverflow)
        );
    }

    #[test]
    fn unmap_preflights_the_whole_range_and_reclaims_page_capacity() {
        let mut memory = GuestMemory::new(2);
        memory
            .map_zeroed(0x1000, PAGE_SIZE, Permissions::READ_WRITE)
            .unwrap();
        memory.write(0x1000, &[42]).unwrap();
        assert_eq!(
            memory.unmap(0x1000, PAGE_SIZE * 2),
            Err(MemoryError::Unmapped { address: 0x2000 })
        );
        let mut byte = [0];
        memory.read(0x1000, &mut byte).unwrap();
        assert_eq!(byte, [42]);
        assert_eq!(memory.mapped_pages(), 1);
        assert_eq!(
            memory.unmap(u64::MAX - 4095, PAGE_SIZE),
            Err(MemoryError::AddressOverflow)
        );
        assert_eq!(
            memory.unmap(0x1001, PAGE_SIZE),
            Err(MemoryError::UnalignedRange)
        );
        memory
            .map_zeroed(0x2000, PAGE_SIZE, Permissions::NONE)
            .unwrap();
        memory.unmap(0x1000, PAGE_SIZE * 2).unwrap();
        assert_eq!(memory.mapped_pages(), 0);
        memory
            .map_zeroed(0x1000, PAGE_SIZE * 2, Permissions::READ)
            .unwrap();
        memory.read(0x1000, &mut byte).unwrap();
        assert_eq!(byte, [0]);
    }
}
