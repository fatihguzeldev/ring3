use super::{DSoundError32, DSoundRuntime32, objects::ObjectKind32};
use crate::{process::HostError, windows::synchronization::EventBinding32};
impl DSoundRuntime32 {
    pub(in crate::process) fn append_process_exit_bindings(
        &self,
        output: &mut Vec<EventBinding32>,
    ) -> Result<(), HostError> {
        let count = self
            .records
            .iter()
            .flatten()
            .map(|object| match &object.kind {
                ObjectKind32::Buffer(buffer) => buffer.notifications.len(),
                ObjectKind32::CaptureBuffer(buffer) => buffer.notifications.len(),
                _ => 0,
            })
            .sum();
        output
            .try_reserve_exact(count)
            .map_err(|_| HostError::DSound(DSoundError32::Allocation))?;
        for object in self.records.iter().flatten() {
            let notifications = match &object.kind {
                ObjectKind32::Buffer(buffer) => &buffer.notifications,
                ObjectKind32::CaptureBuffer(buffer) => &buffer.notifications,
                _ => continue,
            };
            output.extend(
                notifications
                    .iter()
                    .map(|notification| notification.binding),
            );
        }
        Ok(())
    }
}
