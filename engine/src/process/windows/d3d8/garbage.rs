use super::{
    D3D8Runtime32, DispatchError32,
    objects::{Device32, ObjectKind32},
};
use crate::{
    graphics::{
        ResourceId,
        d3d8::{
            api::HResult,
            commands::CommandError,
            device::{Device, DeviceError, DeviceStatus},
            resources::ResourceError,
        },
    },
    memory::GuestAddress,
};

pub(super) struct RetiredDevice32 {
    pub pointer: GuestAddress,
    pub device: Box<Device32>,
    pub closed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Garbage32 {
    Resource {
        device: GuestAddress,
        resource: ResourceId,
    },
    Namespace {
        device: GuestAddress,
        resource: ResourceId,
        namespace: u64,
    },
}

impl D3D8Runtime32 {
    pub(super) fn cleanup_backend(&mut self, epoch: u64) -> Option<&mut Device> {
        let slot = self.device_slots.iter().copied().find(|slot| matches!(self.records[*slot].as_ref().map(|object| &object.kind),Some(ObjectKind32::Device(device)) if device.backend.resources().epoch() == epoch));
        if let Some(slot) = slot {
            let ObjectKind32::Device(device) = &mut self.records[slot].as_mut().unwrap().kind
            else {
                unreachable!()
            };
            return Some(&mut device.backend);
        }
        self.retired_devices
            .iter_mut()
            .find(|retired| retired.device.backend.resources().epoch() == epoch)
            .map(|retired| &mut retired.device.backend)
    }

    pub(super) fn collect_garbage(&mut self) -> Result<(), DispatchError32> {
        let mut index = 0;
        while index < self.garbage.len() {
            let item = self.garbage[index];
            let (device, resource) = match item {
                Garbage32::Resource { device, resource }
                | Garbage32::Namespace {
                    device, resource, ..
                } => (device, resource),
            };
            match item {
                Garbage32::Namespace { namespace, .. } => {
                    let owner = self
                        .cleanup_backend(resource.epoch)
                        .ok_or(HResult::INVALID_CALL)?;
                    match owner
                        .metadata_mut()
                        .free_private_namespace(resource, namespace)
                    {
                        Ok(_) | Err(ResourceError::StaleResource) => {}
                        Err(error) => return Err(DeviceError::Resource(error).into()),
                    }
                }
                Garbage32::Resource { .. } => {
                    if self
                        .retired_devices
                        .iter()
                        .any(|retired| retired.pointer == device)
                    {
                        self.garbage.swap_remove(index);
                        continue;
                    }
                    let has_owner = self.records.iter().flatten().any(|object| {
                        matches!(object.kind,ObjectKind32::Resource(owned) if owned.resource==resource)
                    });
                    if self.locks.contains_resource(resource) || has_owner {
                        index += 1;
                        continue;
                    }
                    let owner = self
                        .cleanup_backend(resource.epoch)
                        .ok_or(HResult::INVALID_CALL)?;
                    if owner.resources().desc(resource) == Err(ResourceError::StaleResource) {
                        self.garbage.swap_remove(index);
                        continue;
                    }
                    if owner.queue().is_draining() || owner.queue().is_sealed() {
                        index += 1;
                        continue;
                    }
                    let prepared = match owner.prepare_destroy_resources(&[resource]) {
                        Ok(prepared) => prepared,
                        Err(
                            DeviceError::PendingReadback
                            | DeviceError::Command(CommandError::Backpressure),
                        ) => {
                            index += 1;
                            continue;
                        }
                        Err(error) => return Err(error.into()),
                    };
                    owner.commit_destruction(prepared)?;
                }
            }
            self.garbage.swap_remove(index);
        }
        for retired in &mut self.retired_devices {
            if retired.closed {
                continue;
            }
            if retired.device.backend.status() == DeviceStatus::Removed {
                retired.closed = true;
                continue;
            }
            if retired.device.backend.queue().is_draining()
                || retired.device.backend.queue().is_sealed()
            {
                continue;
            }
            let prepared = match retired.device.backend.prepare_close() {
                Ok(prepared) => prepared,
                Err(DeviceError::Command(CommandError::Backpressure)) => continue,
                Err(error) => return Err(error.into()),
            };
            retired.device.backend.commit_close(prepared)?;
            retired.closed = true;
        }
        Ok(())
    }
}
