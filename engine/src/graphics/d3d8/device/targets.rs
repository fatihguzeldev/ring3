use super::super::resources::{ResourceDesc, SurfaceRef, Usage};
use super::{Device, DeviceError};

pub struct PreparedTargets {
    color: SurfaceRef,
    depth: Option<SurfaceRef>,
    width: u32,
    height: u32,
    generation: u64,
    revision: u64,
    recording_revision: u64,
    recording: bool,
}

impl Device {
    pub fn prepare_targets(
        &self,
        color: Option<SurfaceRef>,
        depth: Option<SurfaceRef>,
    ) -> Result<PreparedTargets, DeviceError> {
        self.available()?;
        if self.state.revision() == u64::MAX {
            return Err(DeviceError::IdentityExhausted);
        }
        let color = color.unwrap_or(self.color_target);
        let desc = self.resources.desc(color.resource)?;
        let info = self.resources.info(color)?;
        if desc.format().is_depth()
            || !desc.usage().contains(Usage::RENDER_TARGET)
            || info.depth != 1
            || self.resources.is_locked(color.resource)?
        {
            return Err(DeviceError::InvalidCall);
        }
        self.ensure_resource_idle(color.resource)?;
        let samples = match desc {
            ResourceDesc::Surface(desc) => desc.multisample,
            _ => 0,
        };
        if let Some(depth) = depth {
            let desc = self.resources.desc(depth.resource)?;
            let extent = self.resources.info(depth)?;
            let depth_samples = match desc {
                ResourceDesc::Surface(desc) => desc.multisample,
                _ => 0,
            };
            if !desc.format().is_depth()
                || !desc.usage().contains(Usage::DEPTH_STENCIL)
                || extent.width < info.width
                || extent.height < info.height
                || samples != depth_samples
                || self.resources.is_locked(depth.resource)?
            {
                return Err(DeviceError::InvalidCall);
            }
            self.ensure_resource_idle(depth.resource)?;
        }
        Ok(PreparedTargets {
            color,
            depth,
            width: info.width,
            height: info.height,
            generation: self.generation,
            revision: self.state.revision(),
            recording_revision: self.state.recording_revision(),
            recording: self.state.is_recording(),
        })
    }

    pub fn can_commit_targets(&self, prepared: &PreparedTargets) -> Result<(), DeviceError> {
        self.available()?;
        if prepared.generation != self.generation
            || prepared.revision != self.state.revision()
            || prepared.recording_revision != self.state.recording_revision()
            || prepared.recording != self.state.is_recording()
        {
            return Err(DeviceError::InvalidCall);
        }
        self.resources.info(prepared.color)?;
        if let Some(depth) = prepared.depth {
            self.resources.info(depth)?;
        }
        Ok(())
    }

    pub fn commit_targets(&mut self, prepared: PreparedTargets) -> Result<(), DeviceError> {
        self.can_commit_targets(&prepared)?;
        self.state
            .set_target_size(prepared.width, prepared.height)?;
        self.color_target = prepared.color;
        self.depth_target = prepared.depth;
        Ok(())
    }
}
