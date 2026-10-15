use super::super::{
    commands::{Command, DeviceEvent, PreparedGroup, Reset, SurfaceVersion},
    resources::{PreparedReplacement, SurfaceRef},
    state::DeviceState,
};
use super::context::{color_description, depth_description};
use super::{Device, DeviceDescription, DeviceError, DeviceStatus, Outcome};
use crate::graphics::ResourceId;

pub struct PreparedReset {
    resources: PreparedReplacement,
    commands: PreparedGroup,
    description: DeviceDescription,
    state: DeviceState,
    back_buffers: Vec<SurfaceRef>,
    depth: Option<SurfaceRef>,
    generation: u64,
}

impl PreparedReset {
    pub fn back_buffers(&self) -> &[SurfaceRef] {
        &self.back_buffers
    }
    pub fn depth(&self) -> Option<SurfaceRef> {
        self.depth
    }
    pub fn description(&self) -> DeviceDescription {
        self.description
    }
}

pub struct PreparedDestruction {
    resources: PreparedReplacement,
    commands: PreparedGroup,
    ids: Vec<ResourceId>,
}
pub struct PreparedClose {
    commands: PreparedGroup,
    generation: u64,
}

impl Device {
    pub fn reset_failed(&mut self) {
        if self.status == DeviceStatus::Operational {
            self.status = DeviceStatus::Resettable;
        }
    }
    pub fn prepare_reset(
        &self,
        description: DeviceDescription,
    ) -> Result<PreparedReset, DeviceError> {
        self.prepare_reset_with_disposal(description, &[])
    }

    pub fn prepare_reset_with_disposal(
        &self,
        description: DeviceDescription,
        disposal: &[ResourceId],
    ) -> Result<PreparedReset, DeviceError> {
        description.validate()?;
        if self.state.is_recording()
            || self.status == DeviceStatus::Removed
            || description.behavior != self.description.behavior
        {
            return Err(DeviceError::InvalidCall);
        }
        let generation = self
            .generation
            .checked_add(1)
            .ok_or(DeviceError::IdentityExhausted)?;
        let mut old = Vec::new();
        old.try_reserve_exact(
            self.back_buffers.len() + usize::from(self.implicit_depth.is_some()) + disposal.len(),
        )
        .map_err(|_| DeviceError::Allocation)?;
        old.extend(self.back_buffers.iter().map(|surface| surface.resource));
        old.extend(self.implicit_depth.map(|surface| surface.resource));
        for id in disposal {
            self.resources.desc(*id)?;
            if !old.contains(id) {
                old.push(*id);
            }
        }
        self.resources.can_reset_default_except(&old)?;
        let mut descriptions = Vec::new();
        descriptions
            .try_reserve_exact(
                description.back_buffers as usize + usize::from(description.depth.is_some()),
            )
            .map_err(|_| DeviceError::Allocation)?;
        descriptions.resize(
            description.back_buffers as usize,
            color_description(description),
        );
        descriptions.extend(
            description
                .depth
                .map(|format| depth_description(description, format)),
        );
        let resources = self.resources.prepare_replace(&old, &descriptions)?;
        let mut back_buffers = Vec::new();
        back_buffers
            .try_reserve_exact(description.back_buffers as usize)
            .map_err(|_| DeviceError::Allocation)?;
        back_buffers.extend(
            resources.ids()[..description.back_buffers as usize]
                .iter()
                .map(|resource| SurfaceRef {
                    resource: *resource,
                    subresource: 0,
                }),
        );
        let depth = description.depth.map(|_| SurfaceRef {
            resource: resources.ids()[description.back_buffers as usize],
            subresource: 0,
        });
        let mut versions = Vec::new();
        versions
            .try_reserve_exact(back_buffers.len())
            .map_err(|_| DeviceError::Allocation)?;
        versions.extend(back_buffers.iter().map(|surface| SurfaceVersion {
            surface: *surface,
            version: 1,
        }));
        let mut commands = Vec::new();
        commands
            .try_reserve_exact(old.len() + resources.ids().len() + 1)
            .map_err(|_| DeviceError::Allocation)?;
        for id in old {
            commands.push(Command::DestroyResource(self.resource_version(id)?));
        }
        for (index, id) in resources.ids().iter().enumerate() {
            commands.push(Command::CreateResource {
                id: *id,
                desc: resources.desc(index),
                subresources: resources.infos(index),
            });
        }
        commands.push(Command::Reset(Reset {
            generation,
            width: description.width,
            height: description.height,
            format: description.format,
            multisample: description.multisample,
            back_buffers: &versions,
            depth: depth.map(|surface| SurfaceVersion {
                surface,
                version: 1,
            }),
        }));
        let commands = self.queue.prepare(&commands)?;
        let mut state = DeviceState::new(
            description.width,
            description.height,
            description.depth.is_some(),
        )?;
        state.set_render_state(153, u32::from(description.behavior & 0xe0 == 0x20))?;
        Ok(PreparedReset {
            resources,
            commands,
            description,
            state,
            back_buffers,
            depth,
            generation,
        })
    }

    pub fn can_commit_reset(&self, prepared: &PreparedReset) -> Result<(), DeviceError> {
        if self.state.is_recording()
            || self.status == DeviceStatus::Removed
            || self.generation.checked_add(1) != Some(prepared.generation)
        {
            return Err(DeviceError::InvalidCall);
        }
        self.resources.can_commit_replace(&prepared.resources)?;
        self.queue.can_commit(&prepared.commands)?;
        Ok(())
    }

    pub fn commit_reset(&mut self, prepared: PreparedReset) -> Result<Outcome<()>, DeviceError> {
        self.can_commit_reset(&prepared)?;
        let progress = self.queue.commit(prepared.commands)?;
        self.resources.commit_replace(prepared.resources)?;
        self.description = prepared.description;
        self.state = prepared.state;
        self.back_buffers = prepared.back_buffers;
        self.implicit_depth = prepared.depth;
        self.color_target = self.back_buffers[0];
        self.depth_target = self.implicit_depth;
        self.generation = prepared.generation;
        self.status = DeviceStatus::Operational;
        self.scene = false;
        self.vertex = None;
        self.fvf = None;
        self.pixel = None;
        self.recorded_vertex = None;
        self.recorded_fvf = None;
        self.recorded_pixel = None;
        self.blocks.clear();
        self.readbacks.clear();
        self.emitted_pipeline = None;
        self.presented = None;
        Ok(Outcome::new((), progress))
    }

    pub fn reset(&mut self, description: DeviceDescription) -> Result<Outcome<()>, DeviceError> {
        let prepared = self.prepare_reset(description)?;
        self.commit_reset(prepared)
    }

    pub fn observe_status(
        &mut self,
        generation: u64,
        status: DeviceStatus,
    ) -> Result<Outcome<()>, DeviceError> {
        if generation != self.generation {
            return Err(DeviceError::StaleReadback);
        }
        let event = match (self.status, status) {
            (DeviceStatus::Operational, DeviceStatus::Lost) => DeviceEvent::Lost,
            (DeviceStatus::Lost, DeviceStatus::Resettable) => DeviceEvent::Resettable,
            (_, DeviceStatus::Removed) if self.status != DeviceStatus::Removed => {
                DeviceEvent::Removed
            }
            (previous, next) if previous == next => {
                return Ok(Outcome::new((), super::super::commands::Progress::Queued));
            }
            _ => return Err(DeviceError::InvalidCall),
        };
        let progress = self
            .queue
            .submit(&Command::DeviceEvent { generation, event })?;
        self.status = status;
        if matches!(status,DeviceStatus::Lost|DeviceStatus::Removed) {self.readbacks.clear();}
        Ok(Outcome::new((), progress))
    }

    pub fn prepare_destroy_resources(
        &self,
        ids: &[ResourceId],
    ) -> Result<PreparedDestruction, DeviceError> {
        let mut owned = Vec::new();
        owned
            .try_reserve_exact(ids.len())
            .map_err(|_| DeviceError::Allocation)?;
        let mut commands = Vec::new();
        commands
            .try_reserve_exact(ids.len())
            .map_err(|_| DeviceError::Allocation)?;
        for id in ids {
            self.validate_destroy(*id)?;
            owned.push(*id);
            commands.push(Command::DestroyResource(self.resource_version(*id)?));
        }
        let resources = self.resources.prepare_replace(ids, &[])?;
        let commands = self.queue.prepare(&commands)?;
        Ok(PreparedDestruction {
            resources,
            commands,
            ids: owned,
        })
    }

    pub fn can_commit_destruction(
        &self,
        prepared: &PreparedDestruction,
    ) -> Result<(), DeviceError> {
        for id in &prepared.ids {
            self.validate_destroy(*id)?;
        }
        self.resources.can_commit_replace(&prepared.resources)?;
        self.queue.can_commit(&prepared.commands)?;
        Ok(())
    }

    pub fn commit_destruction(
        &mut self,
        prepared: PreparedDestruction,
    ) -> Result<Outcome<()>, DeviceError> {
        self.can_commit_destruction(&prepared)?;
        let progress = self.queue.commit(prepared.commands)?;
        self.resources.commit_replace(prepared.resources)?;
        Ok(Outcome::new((), progress))
    }

    fn validate_destroy(&self, id: ResourceId) -> Result<(), DeviceError> {
        if self
            .back_buffers
            .iter()
            .any(|surface| surface.resource == id)
            || self
                .implicit_depth
                .is_some_and(|surface| surface.resource == id)
            || self.color_target.resource == id
            || self
                .depth_target
                .is_some_and(|surface| surface.resource == id)
            || self.resources.is_locked(id)?
        {
            return Err(DeviceError::InvalidCall);
        }
        self.ensure_resource_idle(id)
    }

    pub fn prepare_close(&self) -> Result<PreparedClose, DeviceError> {
        if self.status == DeviceStatus::Removed {
            return Err(DeviceError::Removed);
        }
        Ok(PreparedClose {
            commands: self.queue.prepare(&[Command::DeviceEvent {
                generation: self.generation,
                event: DeviceEvent::Removed,
            }])?,
            generation: self.generation,
        })
    }

    pub fn can_commit_close(&self, prepared: &PreparedClose) -> Result<(), DeviceError> {
        if self.status == DeviceStatus::Removed || self.generation != prepared.generation {
            return Err(DeviceError::Removed);
        }
        self.queue.can_commit(&prepared.commands)?;
        Ok(())
    }

    pub fn commit_close(&mut self, prepared: PreparedClose) -> Result<Outcome<()>, DeviceError> {
        self.can_commit_close(&prepared)?;
        let progress = self.queue.commit(prepared.commands)?;
        self.status = DeviceStatus::Removed;
        Ok(Outcome::new((), progress))
    }

    pub fn draw_patch(&self) -> Result<(), DeviceError> {
        self.available()?;
        Err(DeviceError::NotAvailable)
    }
    pub fn delete_patch(&self, _handle: u32) -> Result<(), DeviceError> {
        self.available()?;
        Err(DeviceError::InvalidCall)
    }
}
