use super::*;
use crate::{
    memory::{Access, GuestAddress, WordWrite32},
    process::EngineInstance,
};

impl EngineInstance {
    pub(super) fn set_heap_information(&mut self, args: &[u32]) -> Result<u32, MemoryError32> {
        match args[1] {
            0 => {
                if args[3] < 4 {
                    return Err(MemoryError32::InvalidParameter);
                }
                let mut mode = [0; 4];
                self.memory
                    .as_ref()
                    .unwrap()
                    .read(GuestAddress(args[2]), &mut mode)?;
                self.windows_memory
                    .heaps
                    .enable_small_blocks(args[0], u32::from_le_bytes(mode))?;
            }
            1 => {
                if args[2] != 0 || args[3] != 0 {
                    return Err(MemoryError32::InvalidParameter);
                }
                self.windows_memory.heaps.enable_termination_on_corruption();
            }
            _ => return Err(MemoryError32::InvalidParameter),
        }
        Ok(1)
    }
    pub(super) fn query_heap_information(&mut self, args: &[u32]) -> Result<u32, MemoryError32> {
        if args[1] != 0 {
            return Err(MemoryError32::InvalidParameter);
        }
        let mode = self.windows_memory.heaps.compatibility(args[0])?;
        let mut writes = [WordWrite32 {
            address: GuestAddress(0),
            value: 0,
        }; 2];
        let mut count = 0;
        if args[4] != 0 {
            writes[count] = WordWrite32 {
                address: GuestAddress(args[4]),
                value: 4,
            };
            count += 1;
        }
        if args[3] >= 4 {
            writes[count] = WordWrite32 {
                address: GuestAddress(args[2]),
                value: mode,
            };
            count += 1;
        }
        if count != 0 {
            let error_write = args[3] < 4
                && self.module_teb_address().is_some()
                && self
                    .current_windows_thread()
                    .map_err(|_| MemoryError32::NoAccess)?
                    .last_error()
                    != 122;
            self.memory
                .as_ref()
                .unwrap()
                .ensure_transaction_versions(1 + u64::from(error_write))?;
        }
        self.memory
            .as_mut()
            .unwrap()
            .write_words32(&writes[..count])?;
        if args[3] < 4 {
            return Err(MemoryError32::InsufficientBuffer);
        }
        Ok(1)
    }
    pub(super) fn initialize_slist_head(&mut self, pointer: u32) -> Result<u32, MemoryError32> {
        if !pointer.is_multiple_of(8) {
            return Err(MemoryError32::InvalidParameter);
        }
        self.memory
            .as_ref()
            .unwrap()
            .check_span(GuestAddress(pointer), 8, Access::Write)?;
        self.memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(pointer), &[0; 8])?;
        Ok(0)
    }
}
