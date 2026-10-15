use super::PreparedLifetimeChange32;
use crate::{
    memory::AddressSpace,
    process::windows::d3d8::{D3D8Runtime32, garbage::RetiredDevice32, objects::ObjectKind32},
};
impl D3D8Runtime32 {
    pub(in crate::process::windows::d3d8) fn commit_lifetime(
        &mut self,
        memory: &mut AddressSpace,
        prepared: PreparedLifetimeChange32,
    ) {
        let destroyed = self
            .com
            .commit_lifetime_changes(memory, prepared.com)
            .expect("exclusive prepared lifetime remains current");
        for (resource, token) in &prepared.locks.tokens {
            self.cleanup_backend(resource.epoch)
                .expect("disposed lock has its owning device")
                .metadata_mut()
                .cancel_lock(*resource, *token)
                .expect("disposed lock token remains live until atomic reclamation");
        }
        self.locks.commit_disposal(&prepared.locks);
        self.deferred_private_releases
            .extend(prepared.external_releases);
        self.garbage.extend(prepared.garbage);
        for identity in destroyed {
            let object = self.records[identity.payload as usize]
                .take()
                .expect("prepared identity has its typed payload");
            if let ObjectKind32::Device(device) = object.kind {
                self.device_slots
                    .retain(|slot| *slot != identity.payload as usize);
                self.retired_devices.push(RetiredDevice32 {
                    pointer: object.pointer,
                    device,
                    closed: false,
                });
            }
        }
    }
}
