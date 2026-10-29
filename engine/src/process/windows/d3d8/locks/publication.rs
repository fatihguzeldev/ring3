use super::super::{D3D8Runtime32, DispatchError32, memory::check_output};
use crate::{
    graphics::{
        ResourceId,
        d3d8::{
            api::HResult,
            device::DeviceError,
            resources::{Box3, LockFlags, ResourceError, SurfaceRef},
        },
    },
    memory::{AddressSpace, GuestAddress},
};

impl D3D8Runtime32 {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::process::windows::d3d8) fn lock_buffer_guest(
        &mut self,
        memory: &mut AddressSpace,
        device: GuestAddress,
        id: ResourceId,
        offset: u32,
        size: u32,
        flags: LockFlags,
        output: u32,
    ) -> Result<(), DispatchError32> {
        check_output(memory, output, 4)?;
        self.locks.preflight(memory)?;
        let result = self
            .device_mut(device)?
            .lock_buffer(id, offset, size, flags);
        let info = match result {
            Err(DeviceError::Resource(ResourceError::ReadbackRequired)) => {
                return self.lock_readback(
                    device,
                    SurfaceRef {
                        resource: id,
                        subresource: 0,
                    },
                );
            }
            result => result?,
        };
        self.publish_lock(memory, device, id, info, output, None)
    }

    #[allow(clippy::too_many_arguments)]
    pub(in crate::process::windows::d3d8) fn lock_image(
        &mut self,
        memory: &mut AddressSpace,
        device: GuestAddress,
        surface: SurfaceRef,
        region: Option<Box3>,
        flags: LockFlags,
        output: u32,
        volume: bool,
    ) -> Result<(), DispatchError32> {
        check_output(memory, output, if volume { 12 } else { 8 })?;
        self.locks.preflight(memory)?;
        let result = self.device_mut(device)?.lock_box(surface, region, flags);
        let info = match result {
            Err(DeviceError::Resource(ResourceError::ReadbackRequired)) => {
                return self.lock_readback(device, surface);
            }
            result => result?,
        };
        self.publish_lock(memory, device, surface.resource, info, output, Some(volume))
    }

    fn lock_readback(
        &mut self,
        device: GuestAddress,
        surface: SurfaceRef,
    ) -> Result<(), DispatchError32> {
        let id = self
            .device_mut(device)?
            .request_lock_readback(surface)?
            .value;
        Err(DispatchError32::LockReadback { id })
    }

    fn publish_lock(
        &mut self,
        memory: &mut AddressSpace,
        device: GuestAddress,
        id: ResourceId,
        info: crate::graphics::d3d8::resources::LockInfo,
        output: u32,
        volume: Option<bool>,
    ) -> Result<(), DispatchError32> {
        let slot = self.record_slot(device)?;
        let super::super::objects::ObjectKind32::Device(device_record) =
            &self.records[slot].as_ref().unwrap().kind
        else {
            return Err(HResult::INVALID_CALL.into());
        };
        let resources = device_record.backend.resources();
        let bytes = if volume.is_none() {
            resources.lock_backing_bytes(id, info.token)
        } else {
            resources.lock_bytes(id, info.token)
        }
        .map_err(DeviceError::from)?;
        let result = self.locks.publish(memory, id, info, bytes, output, volume);
        if result.is_err() {
            self.device_mut(device)?
                .resources_mut()?
                .cancel_lock(id, info.token)
                .map_err(DeviceError::from)?;
        }
        result
    }

    pub(in crate::process::windows::d3d8) fn unlock_guest(
        &mut self,
        memory: &mut AddressSpace,
        device: GuestAddress,
        surface: SurfaceRef,
    ) -> Result<(), DispatchError32> {
        let index = self
            .locks
            .entries
            .iter()
            .rposition(|entry| {
                entry.resource == surface.resource && entry.info.subresource == surface.subresource
            })
            .ok_or(HResult::INVALID_CALL)?;
        self.locks
            .validate_mapping(memory, &self.locks.entries[index])?;
        let range = self.locks.entries[index].range;
        let last_mapping_lock = !self
            .locks
            .entries
            .iter()
            .enumerate()
            .any(|(other, entry)| other != index && entry.range == range);
        if last_mapping_lock && memory.transaction_version() == u64::MAX {
            return Err(crate::memory::MemoryError::VersionExhausted.into());
        }
        let info = self.locks.entries[index].info;
        let bytes = if info.flags.contains(LockFlags::READ_ONLY) {
            None
        } else {
            Some(self.locks.staging_bytes(memory, index)?)
        };
        self.device_mut(device)?
            .unlock(surface.resource, info.token, bytes.as_deref())?;
        if last_mapping_lock && !self.locks.retain_staging(memory, range) {
            memory.unmap(range)?;
        }
        self.locks.entries.remove(index);
        Ok(())
    }

    pub(in crate::process::windows::d3d8) fn preload_resource(
        &self,
        device: GuestAddress,
        id: ResourceId,
    ) -> Result<(), DispatchError32> {
        self.device(device)?
            .resources()
            .desc(id)
            .map_err(DeviceError::from)?;
        Ok(())
    }
}
