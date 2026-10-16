use super::{D3D8Error32, D3D8Runtime32, garbage::RetiredDevice32, objects::ObjectKind32};
use crate::process::HostError;
impl D3D8Runtime32 {
    pub(in crate::process) fn prepare_process_exit(&mut self) -> Result<(), HostError> {
        self.retired_devices
            .try_reserve_exact(self.device_slots.len())
            .map_err(|_| HostError::D3D8(D3D8Error32::Allocation))
    }
    pub(in crate::process) fn commit_process_exit(&mut self) {
        for &slot in &self.device_slots {
            let object = self.records[slot]
                .take()
                .expect("live device slot owns its record");
            let ObjectKind32::Device(device) = object.kind else {
                unreachable!()
            };
            self.retired_devices.push(RetiredDevice32 {
                pointer: object.pointer,
                device,
                closed: false,
            });
        }
        self.device_slots.clear();
        for record in &mut self.records {
            *record = None;
        }
        self.garbage.clear();
        self.locks.terminate_process();
        self.private_callbacks.clear();
        self.deferred_private_releases.clear();
        self.readback_input = None;
    }
}
