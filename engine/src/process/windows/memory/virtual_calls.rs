use super::*;
use crate::{
    memory::{Access, GuestAddress, Permissions},
    process::EngineInstance,
    windows::memory::{MEM_COMMIT, MEM_PRIVATE, protection, protection_value, span},
};

impl EngineInstance {
    pub(in crate::process::windows) fn memory_allocate(
        &mut self,
        args: &[u32],
    ) -> Result<u32, MemoryError32> {
        let external = self
            .guest_address_reservations()
            .map_err(|_| MemoryError32::Allocation)?;
        self.windows_memory.virtual_memory.allocate(
            self.memory.as_mut().ok_or(MemoryError32::InvalidHandle)?,
            &external,
            args[0],
            args[1],
            args[2],
            args[3],
        )
    }
    pub(in crate::process::windows) fn memory_free(
        &mut self,
        args: &[u32],
    ) -> Result<u32, MemoryError32> {
        self.windows_memory.virtual_memory.free(
            self.memory.as_mut().ok_or(MemoryError32::InvalidHandle)?,
            args[0],
            args[1],
            args[2],
        )?;
        Ok(1)
    }
    pub(in crate::process::windows) fn memory_protect(
        &mut self,
        args: &[u32],
    ) -> Result<u32, MemoryError32> {
        let [address, size, protect, output] =
            *<&[u32; 4]>::try_from(args).map_err(|_| MemoryError32::InvalidParameter)?;
        if output == 0 {
            return Err(MemoryError32::NoAccess);
        }
        self.memory
            .as_ref()
            .ok_or(MemoryError32::InvalidHandle)?
            .check_span(GuestAddress(output), 4, Access::Write)
            .map_err(|error| match error {
                crate::memory::MemoryError::InvalidRange => MemoryError32::NoAccess,
                error => error.into(),
            })?;
        if self
            .windows_memory
            .virtual_memory
            .allocation_base(address)
            .is_some()
        {
            self.windows_memory.virtual_memory.protect(
                self.memory.as_mut().ok_or(MemoryError32::InvalidHandle)?,
                address,
                size,
                protect,
                GuestAddress(output),
            )?;
            return Ok(1);
        }
        let range = span(address, size)?;
        let permissions = protection(protect)?;
        let mapped = self
            .validate_file_mapping_protection(range, permissions.without_guard())
            .map_err(|error| {
                if error == crate::files::FileError32::AccessDenied {
                    MemoryError32::InvalidParameter
                } else {
                    MemoryError32::InvalidAddress
                }
            })?;
        let resource = self
            .validate_resource_mapping_protection(range, permissions.without_guard())
            .map_err(|_| MemoryError32::InvalidParameter)?;
        if !mapped
            && !resource
            && !self.module_image_contains_range(range)
            && !self.image.is_some_and(|image| {
                address >= image.image_base
                    && u64::from(address) + u64::from(size)
                        <= u64::from(image.image_base) + u64::from(image.image_size)
            })
        {
            return Err(MemoryError32::InvalidAddress);
        }
        let mapping_info = if mapped {
            self.file_mapping_memory_info(address)
                .map_err(|_| MemoryError32::InvalidAddress)?
        } else {
            None
        };
        if matches!(protect & !0x100, 8 | 0x80) && !mapped {
            return Err(MemoryError32::InvalidParameter);
        }
        let memory = self.memory.as_mut().ok_or(MemoryError32::InvalidHandle)?;
        let old = mapping_info.map_or(
            protection_value(
                memory
                    .permissions_at(GuestAddress(range.first as u32 * crate::memory::PAGE_SIZE))?,
            ),
            |info| info.protection,
        );
        let word = crate::memory::WordWrite32 {
            address: GuestAddress(output),
            value: old,
        };
        if mapped && matches!(protect & !0x100, 8 | 0x80) {
            memory.protect_copy_with_word_write32(range, permissions, word)?;
        } else {
            memory.protect_with_word_write32(range, permissions, word)?;
        }
        if mapped {
            self.record_file_mapping_protection(range, protect);
        }
        if resource {
            self.commit_resource_mapping_protection(range);
        }
        Ok(1)
    }
    pub(super) fn memory_query(&mut self, args: &[u32]) -> Result<u32, MemoryError32> {
        if args[2] < 28 {
            return Err(MemoryError32::InvalidParameter);
        }
        let memory = self.memory.as_ref().ok_or(MemoryError32::InvalidHandle)?;
        memory.check_span(GuestAddress(args[1]), 28, Access::Write)?;
        let mapped_info = self
            .file_mapping_memory_info(args[0])
            .map_err(|_| MemoryError32::InvalidAddress)?;
        let mut info = if let Some(info) = mapped_info {
            info
        } else {
            self.windows_memory.virtual_memory.query(memory, args[0])?
        };
        if mapped_info.is_none()
            && self
                .windows_memory
                .virtual_memory
                .allocation_base(args[0])
                .is_none()
        {
            let page = span(args[0], 1)?;
            for module in self.loaded_windows_modules() {
                if args[0] >= module.base()
                    && u64::from(args[0])
                        < u64::from(module.base()) + u64::from(module.image_size())
                {
                    info.allocation_base = module.base();
                    info.kind = 0x1000000;
                    info.size = info
                        .size
                        .min(module.base() + module.image_size() - info.base);
                    break;
                }
            }
            if info.kind == MEM_PRIVATE
                && self
                    .validate_file_mapping_protection(page, Permissions::READ)
                    .is_ok_and(|mapped| mapped)
            {
                info.kind = 0x40000;
            }
            if info.state == MEM_COMMIT && info.kind == MEM_PRIVATE {
                let external = self
                    .guest_address_reservations()
                    .map_err(|_| MemoryError32::Allocation)?;
                if let Some(range) = external.into_iter().find(|range| {
                    range
                        .indices()
                        .contains(&(args[0] as usize / crate::memory::PAGE_SIZE as usize))
                }) {
                    info.allocation_base = range.first as u32 * crate::memory::PAGE_SIZE;
                    info.size = info.size.min(
                        ((range.first + range.count) * crate::memory::PAGE_SIZE as usize) as u32
                            - info.base,
                    );
                }
            }
        }
        if let Some((base, extent, layout)) = self
            .resource_mapping_info(args[0])
            .map_err(|_| MemoryError32::InvalidAddress)?
        {
            info.allocation_base = base;
            info.allocation_protection = 2;
            info.kind = match layout {
                crate::loader::DataFileLayout32::File => 0x40000,
                crate::loader::DataFileLayout32::Image => 0x1000000,
            };
            if info.state != MEM_COMMIT {
                info.state = crate::windows::memory::MEM_RESERVE;
                info.protection = 0;
            }
            info.base = info.base.max(base);
            info.size = info.size.min(base + extent - info.base);
        }
        self.memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(args[1]), &info.bytes())?;
        Ok(28)
    }
}
