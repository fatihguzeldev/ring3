use super::super::{
    commands::{Command, Progress, ResourceVersion, Upload},
    resources::{Box3, LockFlags, LockInfo, ResourceDesc, SurfaceRef},
};
use super::{Device, DeviceError, Outcome};
use crate::graphics::ResourceId;

impl Device {
    pub fn create_resource(
        &mut self,
        desc: ResourceDesc,
    ) -> Result<Outcome<ResourceId>, DeviceError> {
        let prepared = self.prepare_resource(desc)?;
        self.commit_resource(prepared)
    }

    pub fn destroy_resource(&mut self, id: ResourceId) -> Result<Outcome<()>, DeviceError> {
        let prepared = self.prepare_destroy_resources(&[id])?;
        self.commit_destruction(prepared)
    }

    pub fn lock_buffer(
        &mut self,
        id: ResourceId,
        offset: u32,
        size: u32,
        flags: LockFlags,
    ) -> Result<LockInfo, DeviceError> {
        self.available()?;
        self.ensure_resource_idle(id)?;
        Ok(self.resources.lock_buffer(id, offset, size, flags)?)
    }

    pub fn lock_box(
        &mut self,
        surface: SurfaceRef,
        region: Option<Box3>,
        flags: LockFlags,
    ) -> Result<LockInfo, DeviceError> {
        self.available()?;
        self.ensure_resource_idle(surface.resource)?;
        Ok(self.resources.lock_box(surface, region, flags)?)
    }

    pub fn unlock(
        &mut self,
        id: ResourceId,
        token: u64,
        bytes: Option<&[u8]>,
    ) -> Result<Outcome<()>, DeviceError> {
        self.available()?;
        let plan = self.resources.can_unlock(id, token, bytes)?;
        let Some(version) = plan.version else {
            self.resources.unlock(id, token, bytes)?;
            return Ok(Outcome::new((), Progress::Queued));
        };
        let prepared = self.queue.prepare(&[Command::Upload(Upload {
            resource: ResourceVersion { id, version },
            subresource: plan.info.subresource,
            offset: plan.info.offset,
            row_pitch: plan.info.row_pitch,
            slice_pitch: plan.info.slice_pitch,
            row_bytes: plan.row_bytes,
            rows: plan.rows,
            slices: plan.slices,
            bytes: bytes.unwrap(),
        })])?;
        self.queue.can_commit(&prepared)?;
        let progress = self.queue.commit(prepared)?;
        self.resources.unlock(id, token, bytes)?;
        Ok(Outcome::new((), progress))
    }

    pub(super) fn ensure_resource_idle(&self, id: ResourceId) -> Result<(), DeviceError> {
        if self.readbacks.iter().any(|request| {
            request.destination.surface.resource == id || request.source.surface.resource == id
        }) {
            return Err(DeviceError::PendingReadback);
        }
        Ok(())
    }
}
