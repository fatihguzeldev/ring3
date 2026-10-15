use super::super::{D3D8Runtime32, lifetime::LifetimeChange32, objects::ObjectKind32};
use crate::{
    graphics::d3d8::api::HResult,
    memory::{AddressSpace, GuestAddress, WordWrite32},
};

pub(in crate::process::windows::d3d8) struct BindingHolds32 {
    released: Vec<GuestAddress>,
    retained: Vec<GuestAddress>,
}

impl D3D8Runtime32 {
    pub(in crate::process::windows::d3d8) fn binding_hold_delta(
        &self,
        device: GuestAddress,
        released: impl Iterator<Item = u32>,
        retained: impl Iterator<Item = u32>,
    ) -> Result<BindingHolds32, HResult> {
        let released = self.binding_counters(device, released)?;
        let retained = self.binding_counters(device, retained)?;
        let object = self.object(device)?;
        let ObjectKind32::Device(device) = &object.kind else {
            return Err(HResult::INVALID_CALL);
        };
        validate_delta(&device.resource_holds, &released, &retained)?;
        Ok(BindingHolds32 { released, retained })
    }

    fn binding_counters(
        &self,
        device: GuestAddress,
        handles: impl Iterator<Item = u32>,
    ) -> Result<Vec<GuestAddress>, HResult> {
        let mut counters = Vec::new();
        counters
            .try_reserve_exact(25)
            .map_err(|_| HResult::OUT_OF_MEMORY)?;
        for handle in handles {
            let pointer = GuestAddress(handle);
            self.resource(pointer, device)?;
            counters.push(self.counter_pointer(pointer)?);
        }
        Ok(counters)
    }

    pub(in crate::process::windows::d3d8) fn commit_binding_holds(
        &mut self,
        memory: &mut AddressSpace,
        pointer: GuestAddress,
        delta: BindingHolds32,
        writes: &[WordWrite32],
    ) -> Result<(), HResult> {
        let mut hold = Vec::new();
        hold.try_reserve_exact(delta.retained.len())
            .map_err(|_| HResult::OUT_OF_MEMORY)?;
        hold.extend_from_slice(&delta.retained);
        let mut unhold = Vec::new();
        unhold
            .try_reserve_exact(delta.released.len())
            .map_err(|_| HResult::OUT_OF_MEMORY)?;
        unhold.extend_from_slice(&delta.released);
        self.change_lifetime(
            memory,
            LifetimeChange32 {
                hold,
                unhold,
                ..Default::default()
            },
            writes,
        )?;
        let slot = self
            .record_slot(pointer)
            .expect("active device remains alive during binding lifetime commit");
        let ObjectKind32::Device(device) = &mut self.records[slot].as_mut().unwrap().kind else {
            unreachable!()
        };
        for released in delta.released {
            let index = device
                .resource_holds
                .iter()
                .position(|held| *held == released)
                .expect("prepared binding release exists");
            device.resource_holds.swap_remove(index);
        }
        device.resource_holds.extend(delta.retained);
        Ok(())
    }
}

pub(super) fn validate_delta(
    holds: &Vec<GuestAddress>,
    released: &[GuestAddress],
    retained: &[GuestAddress],
) -> Result<(), HResult> {
    for (index, pointer) in released.iter().enumerate() {
        if released[..=index]
            .iter()
            .filter(|value| *value == pointer)
            .count()
            > holds.iter().filter(|value| *value == pointer).count()
        {
            return Err(HResult::INVALID_CALL);
        }
    }
    if holds
        .len()
        .checked_sub(released.len())
        .and_then(|length| length.checked_add(retained.len()))
        .is_none_or(|length| length > holds.capacity())
    {
        return Err(HResult::OUT_OF_MEMORY);
    }
    Ok(())
}
