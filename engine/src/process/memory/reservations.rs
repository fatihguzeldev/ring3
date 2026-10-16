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
        let media = self
            .dsound
            .iter()
            .flat_map(|runtime| runtime.guest_reservations())
            .chain(
                self.dinput
                    .iter()
                    .flat_map(|runtime| runtime.guest_reservations()),
            );
        let graphics = self
            .d3d8
            .iter()
            .flat_map(|runtime| runtime.guest_reservations());
        let com = self.ole32.iter().flat_map(|runtime| {
            [
                runtime.guest_reservation(),
                runtime.activation_scratch_reservation(),
                runtime.rpc_buffer_reservation(),
            ]
        });
        let windows = self
            .user32
            .iter()
            .map(|runtime| runtime.scratch_reservation())
            .chain(
                self.resources
                    .iter()
                    .map(|runtime| runtime.scratch_reservation()),
            )
            .chain(core::iter::once(self.nls.scratch_reservation()))
            .chain(self.activation.iter().map(|runtime| runtime.reservation()));
        media.chain(graphics).chain(com).chain(windows)
            .chain(self.gdi.iter().flat_map(|runtime| runtime.guest_reservations()))
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
