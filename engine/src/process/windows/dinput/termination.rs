use super::{DirectInputError32, DirectInputRuntime32, objects::ObjectKind32};
use crate::{process::HostError, windows::synchronization::EventBinding32};
impl DirectInputRuntime32 {
    pub(in crate::process) fn append_process_exit_bindings(
        &self,
        output: &mut Vec<EventBinding32>,
    ) -> Result<(), HostError> {
        let bindings = self
            .records
            .iter()
            .flatten()
            .filter_map(|object| match &object.kind {
                ObjectKind32::Device(device) => device.notification,
                _ => None,
            });
        output
            .try_reserve_exact(bindings.clone().count())
            .map_err(|_| HostError::DirectInput(DirectInputError32::Allocation))?;
        output.extend(bindings);
        Ok(())
    }
}
