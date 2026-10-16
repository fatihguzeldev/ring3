use crate::{
    memory::{GuestAddress, PAGE_SIZE, PageRange},
    process::{EngineInstance, HostError},
};

impl EngineInstance {
    /// Virtual ownership includes unmapped reservations and configured provider arenas.
    pub(in crate::process) fn guest_address_reservations(
        &self,
    ) -> Result<Vec<PageRange>, HostError> {
        let mut ranges = Vec::new();
        ranges
            .try_reserve_exact(
                self.windows_memory.reservation_count()
                    + self.threads.reservations().count()
                    + self.loaded_windows_modules().len()
                    + self.datafile_reservations().count()
                    + 16,
            )
            .map_err(|_| HostError::Infrastructure)?;
        ranges.extend(self.windows_memory.reservations());
        ranges.extend(self.threads.reservations());
        ranges.extend(self.datafile_reservations());
        for module in self.loaded_windows_modules() {
            ranges.push(
                PageRange::new(
                    GuestAddress(module.base()),
                    module.image_size().div_ceil(PAGE_SIZE),
                )
                .map_err(HostError::Memory)?,
            );
        }
        if let Some(image) = self.image {
            ranges.push(
                PageRange::new(
                    GuestAddress(image.image_base),
                    image.image_size.div_ceil(PAGE_SIZE),
                )
                .map_err(HostError::Memory)?,
            );
        }
        if let Some(thread) = self.initial_thread_reservations() {
            for (start, end) in thread {
                ranges.push(
                    PageRange::new(
                        GuestAddress(start as u32),
                        ((end - start) / u64::from(PAGE_SIZE)) as u32,
                    )
                    .map_err(HostError::Memory)?,
                );
            }
        }
        if let Some(range) = self.process_runtime.reservation() {
            ranges.push(range);
        }
        ranges.extend(self.provider_guest_arenas());
        Ok(ranges)
    }
    pub(in crate::process) fn provider_guest_arenas(&self) -> impl Iterator<Item = PageRange> {
        let mut ranges = [None; 9];
        let mut index = 0;
        if let Some(runtime) = &self.dsound {
            for range in runtime.guest_reservations() {
                ranges[index] = Some(range);
                index += 1;
            }
        }
        if let Some(runtime) = &self.dinput {
            for range in runtime.guest_reservations() {
                ranges[index] = Some(range);
                index += 1;
            }
        }
        if let Some(runtime) = &self.d3d8 {
            for range in runtime.guest_reservations() {
                ranges[index] = Some(range);
                index += 1;
            }
        }
        if let Some(runtime) = &self.ole32 {
            ranges[index] = Some(runtime.guest_reservation());
            index += 1;
        }
        if let Some(runtime) = &self.user32 {
            ranges[index] = Some(runtime.scratch_reservation());
            index += 1;
        }
        if let Some(runtime) = &self.resources {
            ranges[index] = Some(runtime.scratch_reservation());
        }
        ranges.into_iter().flatten()
    }
    pub(in crate::process) fn check_guest_reservation_admission(
        &self,
        new: &[PageRange],
    ) -> Result<(), HostError> {
        let existing = self.guest_address_reservations()?;
        if new
            .iter()
            .any(|new| existing.iter().any(|old| overlaps(*new, *old)))
        {
            return Err(HostError::InvalidRequest);
        }
        Ok(())
    }
}
pub(in crate::process) fn overlaps(left: PageRange, right: PageRange) -> bool {
    left.first < right.first + right.count && right.first < left.first + left.count
}
