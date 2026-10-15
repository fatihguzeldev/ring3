use super::{LifetimeChange32, PreparedUpHolds32};
use crate::process::windows::d3d8::{D3D8Runtime32, DispatchError32, objects::ObjectKind32};
use crate::{
    graphics::d3d8::api::HResult,
    memory::{AddressSpace, GuestAddress},
};
impl D3D8Runtime32 {
    pub(in crate::process::windows::d3d8) fn preflight_up_holds(
        &mut self,
        memory: &AddressSpace,
        pointer: GuestAddress,
        indexed: bool,
    ) -> Result<PreparedUpHolds32, DispatchError32> {
        let state = self.device(pointer)?.state().snapshot();
        let mut released = Vec::new();
        released
            .try_reserve_exact(2)
            .map_err(|_| HResult::OUT_OF_MEMORY)?;
        for handle in [
            state.streams[0].resource,
            if indexed { state.indices } else { 0 },
        ] {
            if handle != 0 {
                released.push(self.counter_pointer(GuestAddress(handle))?);
            }
        }
        let lifetime = self.prepare_lifetime(
            memory,
            LifetimeChange32 {
                unhold: released.clone(),
                ..Default::default()
            },
            &[],
        )?;
        Ok(PreparedUpHolds32 { lifetime, released })
    }
    pub(in crate::process::windows::d3d8) fn complete_up_holds(
        &mut self,
        memory: &mut AddressSpace,
        pointer: GuestAddress,
        prepared: PreparedUpHolds32,
    ) {
        self.commit_lifetime(memory, prepared.lifetime);
        let slot = self
            .record_slot(pointer)
            .expect("draw device remains owned by the caller");
        let ObjectKind32::Device(device) = &mut self.records[slot].as_mut().unwrap().kind else {
            unreachable!()
        };
        for pointer in prepared.released {
            let index = device
                .resource_holds
                .iter()
                .position(|held| *held == pointer)
                .expect("bound stream has a retained COM hold");
            device.resource_holds.remove(index);
        }
    }
}
