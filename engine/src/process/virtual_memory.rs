use super::{EngineInstance, HostError};
use crate::{
    memory::{GuestAddress, MemoryError, PAGE_SIZE, PageRange, Permissions},
    windows::{CallFrame32, ThreadState32},
};

impl EngineInstance {
    pub(super) fn complete_virtual_alloc(
        &mut self,
        frame: CallFrame32,
        thread: ThreadState32,
        size: u32,
    ) -> Result<(), HostError> {
        let bounds = PageRange::new(GuestAddress(0x1000_0000), 0x6000_0000 / PAGE_SIZE)
            .map_err(HostError::Memory)?;
        let mut planned = self
            .memory()?
            .find_free_range(bounds, size.div_ceil(PAGE_SIZE), 65536);
        if let (Ok(range), Some(image)) = (planned, self.image) {
            let range_start = range.first as u64 * u64::from(PAGE_SIZE);
            let range_end = range_start + range.count as u64 * u64::from(PAGE_SIZE);
            let image_start = u64::from(image.image_base);
            let image_end = image_start + u64::from(image.image_size);
            if range_start < image_end && image_start < range_end {
                planned = if image_end >= 0x7000_0000 {
                    Err(MemoryError::Capacity)
                } else {
                    let bounds = PageRange::new(
                        GuestAddress(image_end as u32),
                        (0x7000_0000 - image_end) as u32 / PAGE_SIZE,
                    )
                    .map_err(HostError::Memory)?;
                    self.memory()?
                        .find_free_range(bounds, size.div_ceil(PAGE_SIZE), 65536)
                };
            }
        }
        let range = match planned {
            Ok(range) => range,
            Err(MemoryError::Capacity) => {
                let prepared = Self::prepare_completed_call(frame.complete(0))?;
                self.publish_prepared_call(prepared);
                self.windows_thread = thread.allocation_failed();
                return Ok(());
            }
            Err(error) => return Err(HostError::Memory(error)),
        };
        let address = range.first as u32 * PAGE_SIZE;
        let prepared = Self::prepare_completed_call(frame.complete(address))?;
        self.virtual_allocations
            .try_reserve(1)
            .map_err(|_| HostError::Memory(MemoryError::Allocation))?;
        self.memory
            .as_mut()
            .ok_or(HostError::Closed)?
            .map_zeroed(range, Permissions::READ_WRITE)
            .map_err(HostError::Memory)?;
        self.virtual_allocations.push(range);
        self.publish_prepared_call(prepared);
        self.windows_thread = thread;
        Ok(())
    }

    pub(super) fn complete_virtual_free(
        &mut self,
        frame: CallFrame32,
        thread: ThreadState32,
        address: u32,
    ) -> Result<(), HostError> {
        let Some(index) = self
            .virtual_allocations
            .iter()
            .position(|range| range.first as u32 * PAGE_SIZE == address)
        else {
            let prepared = Self::prepare_completed_call(frame.complete(0))?;
            self.publish_prepared_call(prepared);
            self.windows_thread = thread.release_failed();
            return Ok(());
        };
        let range = self.virtual_allocations[index];
        let prepared = Self::prepare_completed_call(frame.complete(1))?;
        self.memory
            .as_mut()
            .ok_or(HostError::Closed)?
            .unmap(range)
            .map_err(HostError::Memory)?;
        self.virtual_allocations.remove(index);
        self.publish_prepared_call(prepared);
        self.windows_thread = thread;
        Ok(())
    }

    pub(super) fn revoke_virtual_allocations(&mut self, range: PageRange) {
        self.virtual_allocations.retain(|allocation| {
            allocation.first >= range.first + range.count
                || range.first >= allocation.first + allocation.count
        });
    }
}

#[cfg(test)]
#[path = "virtual_memory_tests.rs"]
mod tests;
