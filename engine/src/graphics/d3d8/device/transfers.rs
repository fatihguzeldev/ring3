use super::super::{
    commands::{
        Command, CopyRect, DrawBindings, PipelineId, Progress, ReadbackId, ReadbackKind,
        ReadbackLayout, ReadbackRequest, ResourceVersion, SurfaceVersion, Upload,
    },
    resources::{
        BufferKind, Format, Pool, Rect, ResourceDesc, ResourceError, SurfaceRef, TextureKind,
    },
    shader::Fvf,
};
use super::context::PendingReadback;
use super::{Device, DeviceError, Outcome};
use crate::graphics::ResourceId;

impl Device {
    pub fn update_texture(
        &mut self,
        source: ResourceId,
        destination: ResourceId,
    ) -> Result<Outcome<()>, DeviceError> {
        self.available()?;
        self.ensure_resource_idle(source)?;
        self.ensure_resource_idle(destination)?;
        if !self.resources.can_update_texture(source, destination)? {
            return Ok(Outcome::new((), Progress::Queued));
        }
        let (ResourceDesc::Texture(input), ResourceDesc::Texture(output)) = (
            self.resources.desc(source)?,
            self.resources.desc(destination)?,
        ) else {
            return Err(DeviceError::InvalidCall);
        };
        let faces = if input.kind == TextureKind::Cube {
            6
        } else {
            1
        };
        let offset = input.levels - output.levels;
        let version = self
            .resources
            .version(destination)?
            .checked_add(1)
            .ok_or(DeviceError::IdentityExhausted)?;
        let mut commands = Vec::new();
        commands
            .try_reserve_exact((faces * output.levels) as usize)
            .map_err(|_| DeviceError::Allocation)?;
        for face in 0..faces {
            if self.resources.dirty_region(source, face)?.is_none() {
                continue;
            }
            for level in 0..output.levels {
                let surface = SurfaceRef {
                    resource: source,
                    subresource: face * input.levels + offset + level,
                };
                let info = self.resources.info(surface)?;
                let (block_width, block_height, block_bytes) = input.format.block();
                commands.push(Command::Upload(Upload {
                    resource: ResourceVersion {
                        id: destination,
                        version,
                    },
                    subresource: face * output.levels + level,
                    offset: 0,
                    row_pitch: info.row_pitch,
                    slice_pitch: info.slice_pitch,
                    row_bytes: info.width.div_ceil(block_width) * block_bytes,
                    rows: info.height.div_ceil(block_height),
                    slices: info.depth,
                    bytes: self.resources.bytes(surface)?,
                }));
            }
        }
        let prepared = self.queue.prepare(&commands)?;
        let progress = self.queue.commit(prepared)?;
        self.resources.update_texture(source, destination)?;
        Ok(Outcome::new((), progress))
    }

    pub fn copy_rects(
        &mut self,
        source: SurfaceRef,
        destination: SurfaceRef,
        rectangles: &[Rect],
        points: Option<&[(u32, u32)]>,
    ) -> Result<Outcome<Option<ReadbackId>>, DeviceError> {
        self.available()?;
        self.ensure_resource_idle(source.resource)?;
        self.ensure_resource_idle(destination.resource)?;
        let copies = self.copy_regions(source, destination, rectangles, points)?;
        let source_version = self.surface_version(source)?;
        let destination_version = SurfaceVersion {
            surface: destination,
            version: self
                .resources
                .version(destination.resource)?
                .checked_add(1)
                .ok_or(DeviceError::IdentityExhausted)?,
        };
        let cpu = match self
            .resources
            .prepare_copy_rects(source, destination, rectangles, points)
        {
            Ok(copy) => Some(copy),
            Err(ResourceError::ReadbackRequired) => None,
            Err(error) => return Err(error.into()),
        };
        let needs_readback =
            cpu.is_none() && self.resources.desc(destination.resource)?.pool() != Pool::Default;
        let readback = if needs_readback {
            Some(self.next_readback()?)
        } else {
            None
        };
        if cpu.is_none() {
            self.resources.can_mark_gpu_written(destination)?;
        }
        let copy = Command::Copy {
            source: source_version,
            destination: destination_version,
            rectangles: &copies,
        };
        let prepared = if let Some(id) = readback {
            self.queue.prepare(&[
                copy,
                Command::Readback(ReadbackRequest {
                    id,
                    source: destination_version,
                    kind: ReadbackKind::Copy,
                    expected: self.readback_layout(destination)?,
                }),
            ])?
        } else {
            self.queue.prepare(&[copy])?
        };
        let progress = self.queue.commit(prepared)?;
        if let Some(copy) = cpu {
            self.resources.commit_copy(copy)?;
        } else {
            self.resources.mark_gpu_written(destination)?;
        }
        if let Some(id) = readback {
            self.push_readback(
                id,
                destination_version,
                destination_version,
                ReadbackKind::Copy,
            );
        }
        Ok(Outcome::new(readback, progress))
    }

    pub fn request_lock_readback(
        &mut self,
        surface: SurfaceRef,
    ) -> Result<Outcome<ReadbackId>, DeviceError> {
        self.available()?;
        self.ensure_resource_idle(surface.resource)?;
        if self.resources.is_locked(surface.resource)? || self.resources.bytes(surface).is_ok() {
            return Err(DeviceError::InvalidCall);
        }
        let source = self.surface_version(surface)?;
        let id = self.next_readback()?;
        let progress = self.queue.submit(&Command::Readback(ReadbackRequest {
            id,
            source,
            kind: ReadbackKind::Lock,
            expected: self.readback_layout(surface)?,
        }))?;
        self.push_readback(id, source, source, ReadbackKind::Lock);
        Ok(Outcome::new(id, progress))
    }

    pub fn get_front_buffer(
        &mut self,
        destination: SurfaceRef,
    ) -> Result<Outcome<ReadbackId>, DeviceError> {
        self.get_front_buffer_extent(destination, self.description.width, self.description.height)
    }

    pub fn get_front_buffer_extent(
        &mut self,
        destination: SurfaceRef,
        width: u32,
        height: u32,
    ) -> Result<Outcome<ReadbackId>, DeviceError> {
        self.available()?;
        self.ensure_resource_idle(destination.resource)?;
        let desc = self.resources.desc(destination.resource)?;
        let info = self.resources.info(destination)?;
        if desc.pool() != Pool::SystemMemory
            || desc.format() != Format::A8R8G8B8
            || info.depth != 1
            || info.width != width
            || info.height != height
        {
            return Err(DeviceError::InvalidCall);
        }
        let source = self
            .presented
            .unwrap_or(self.surface_version(self.back_buffers[0])?);
        self.resources.can_mark_gpu_written(destination)?;
        let destination = SurfaceVersion {
            surface: destination,
            version: self
                .resources
                .version(destination.resource)?
                .checked_add(1)
                .ok_or(DeviceError::IdentityExhausted)?,
        };
        let id = self.next_readback()?;
        let progress = self.queue.submit(&Command::Readback(ReadbackRequest {
            id,
            source,
            kind: ReadbackKind::FrontBuffer,
            expected: self.readback_layout(destination.surface)?,
        }))?;
        self.resources.mark_gpu_written(destination.surface)?;
        self.push_readback(id, source, destination, ReadbackKind::FrontBuffer);
        Ok(Outcome::new(id, progress))
    }

    pub fn process_vertices(
        &mut self,
        source_start: u32,
        destination_index: u32,
        vertex_count: u32,
        destination: ResourceId,
        flags: u32,
        bindings: DrawBindings,
    ) -> Result<Outcome<Option<ReadbackId>>, DeviceError> {
        self.available()?;
        if flags & !1 != 0 {
            return Err(DeviceError::InvalidCall);
        }
        let ResourceDesc::Buffer(desc) = self.resources.desc(destination)? else {
            return Err(DeviceError::InvalidCall);
        };
        let BufferKind::Vertex { fvf } = desc.kind else {
            return Err(DeviceError::InvalidCall);
        };
        let format =
            Fvf::parse(fvf).map_err(super::super::shader::registry::RegistryError::from)?;
        if destination_index
            .checked_add(vertex_count)
            .and_then(|end| end.checked_mul(format.stride))
            .is_none_or(|end| end > desc.length)
        {
            return Err(DeviceError::InvalidCall);
        }
        if vertex_count == 0 {
            return Ok(Outcome::new(None, Progress::Queued));
        }
        self.ensure_resource_idle(destination)?;
        if bindings
            .streams
            .iter()
            .flatten()
            .any(|binding| binding.resource.id == destination)
        {
            return Err(DeviceError::InvalidCall);
        }
        self.validate_bindings(&bindings)?;
        self.validate_vertex_range(&bindings, source_start, vertex_count)?;
        let surface = SurfaceRef {
            resource: destination,
            subresource: 0,
        };
        self.resources.can_mark_gpu_written(surface)?;
        let output = ResourceVersion {
            id: destination,
            version: self
                .resources
                .version(destination)?
                .checked_add(1)
                .ok_or(DeviceError::IdentityExhausted)?,
        };
        let id = self.next_readback()?;
        self.drain_retired()?;
        let revision = self.state.revision();
        let (pipeline, changed) = match self.emitted_pipeline {
            Some((old, pipeline)) if old == revision => (pipeline, false),
            _ => (
                PipelineId(
                    self.pipeline_sequence
                        .checked_add(1)
                        .ok_or(DeviceError::IdentityExhausted)?,
                ),
                true,
            ),
        };
        let job = Command::ProcessVertices {
            pipeline,
            bindings,
            source_start,
            destination: output,
            destination_index,
            vertex_count,
            flags,
            destination_fvf: fvf,
        };
        let readback = Command::Readback(ReadbackRequest {
            id,
            source: SurfaceVersion {
                surface,
                version: output.version,
            },
            kind: ReadbackKind::ProcessVertices,
            expected: self.readback_layout(surface)?,
        });
        let prepared = if changed {
            self.queue.prepare(&[
                Command::Pipeline {
                    id: pipeline,
                    state: self.state.snapshot(),
                },
                job,
                readback,
            ])?
        } else {
            self.queue.prepare(&[job, readback])?
        };
        let progress = self.queue.commit(prepared)?;
        self.resources.mark_gpu_written(surface)?;
        let source = SurfaceVersion {
            surface,
            version: output.version,
        };
        self.push_readback(id, source, source, ReadbackKind::ProcessVertices);
        if changed {
            self.pipeline_sequence = pipeline.0;
            self.emitted_pipeline = Some((revision, pipeline));
        }
        Ok(Outcome::new(Some(id), progress))
    }

    pub fn complete_readback(
        &mut self,
        id: ReadbackId,
        generation: u64,
        bytes: &[u8],
    ) -> Result<ReadbackKind, DeviceError> {
        let index = self
            .readbacks
            .iter()
            .position(|request| request.id == id && request.generation == generation)
            .ok_or(DeviceError::StaleReadback)?;
        if generation != self.generation {
            return Err(DeviceError::StaleReadback);
        }
        let request = &self.readbacks[index];
        self.resources.complete_readback(
            request.destination.surface,
            request.destination.version,
            bytes,
        )?;
        let kind = request.kind;
        self.readbacks.remove(index);
        Ok(kind)
    }

    pub fn pending_readback(&self, id: ReadbackId) -> Option<(u64, ReadbackKind, SurfaceVersion)> {
        self.readbacks
            .iter()
            .find(|request| request.id == id)
            .map(|request| (request.generation, request.kind, request.destination))
    }

    pub fn pending_readback_details(
        &self,
        id: ReadbackId,
    ) -> Option<(u64, ReadbackKind, SurfaceVersion, SurfaceVersion)> {
        self.readbacks
            .iter()
            .find(|request| request.id == id)
            .map(|request| {
                (
                    request.generation,
                    request.kind,
                    request.source,
                    request.destination,
                )
            })
    }

    pub fn cancel_readback(
        &mut self,
        id: ReadbackId,
        generation: u64,
        failure: DeviceError,
    ) -> Result<ReadbackKind, DeviceError> {
        if !matches!(
            failure,
            DeviceError::Lost | DeviceError::Removed | DeviceError::Allocation
        ) {
            return Err(DeviceError::InvalidCall);
        }
        let index = self
            .readbacks
            .iter()
            .position(|request| {
                request.id == id
                    && request.generation == generation
                    && generation == self.generation
            })
            .ok_or(DeviceError::StaleReadback)?;
        if failure == DeviceError::Lost {
            self.status = super::DeviceStatus::Lost;
        }
        if failure == DeviceError::Removed {
            self.status = super::DeviceStatus::Removed;
        }
        Ok(self.readbacks.remove(index).kind)
    }

    fn readback_layout(&self, surface: SurfaceRef) -> Result<ReadbackLayout, DeviceError> {
        let info = self.resources.info(surface)?;
        Ok(ReadbackLayout {
            format: self.resources.desc(surface.resource)?.format(),
            width: info.width,
            height: info.height,
            depth: info.depth,
            row_pitch: info.row_pitch,
            slice_pitch: info.slice_pitch,
            length: info.length,
        })
    }

    fn next_readback(&self) -> Result<ReadbackId, DeviceError> {
        if self.readbacks.len() == self.limits.readbacks as usize {
            return Err(DeviceError::Capacity);
        }
        Ok(ReadbackId {
            epoch: self.queue.epoch(),
            sequence: self
                .readback_sequence
                .checked_add(1)
                .ok_or(DeviceError::IdentityExhausted)?,
        })
    }

    fn push_readback(
        &mut self,
        id: ReadbackId,
        source: SurfaceVersion,
        destination: SurfaceVersion,
        kind: ReadbackKind,
    ) {
        self.readback_sequence = id.sequence;
        self.readbacks.push(PendingReadback {
            id,
            generation: self.generation,
            source,
            destination,
            kind,
        });
    }

    fn copy_regions(
        &self,
        source: SurfaceRef,
        destination: SurfaceRef,
        rectangles: &[Rect],
        points: Option<&[(u32, u32)]>,
    ) -> Result<Vec<CopyRect>, DeviceError> {
        let input_desc = self.resources.desc(source.resource)?;
        let output_desc = self.resources.desc(destination.resource)?;
        let input = self.resources.info(source)?;
        let output = self.resources.info(destination)?;
        if source == destination
            || input_desc.format() != output_desc.format()
            || input_desc.format().is_depth()
            || input.depth != 1
            || output.depth != 1
            || matches!(input_desc, ResourceDesc::Buffer(_))
            || matches!(output_desc, ResourceDesc::Buffer(_))
            || self.resources.is_locked(source.resource)?
            || self.resources.is_locked(destination.resource)?
        {
            return Err(DeviceError::InvalidCall);
        }
        let count = rectangles.len().max(1);
        if points.is_some_and(|points| points.len() != count) {
            return Err(DeviceError::InvalidCall);
        }
        let mut copies = Vec::new();
        copies
            .try_reserve_exact(count)
            .map_err(|_| DeviceError::Allocation)?;
        let (bw, bh, _) = input_desc.format().block();
        for index in 0..count {
            let source = rectangles.get(index).copied().unwrap_or(Rect {
                left: 0,
                top: 0,
                right: input.width,
                bottom: input.height,
            });
            let point = points.map_or((source.left, source.top), |points| points[index]);
            if source.left >= source.right
                || source.top >= source.bottom
                || source.right > input.width
                || source.bottom > input.height
                || source.left % bw != 0
                || source.top % bh != 0
                || (source.right != input.width && source.right % bw != 0)
                || (source.bottom != input.height && source.bottom % bh != 0)
                || !point.0.is_multiple_of(bw)
                || !point.1.is_multiple_of(bh)
            {
                return Err(DeviceError::InvalidCall);
            }
            let right = point
                .0
                .checked_add(source.right - source.left)
                .ok_or(DeviceError::InvalidCall)?;
            let bottom = point
                .1
                .checked_add(source.bottom - source.top)
                .ok_or(DeviceError::InvalidCall)?;
            if right > output.width
                || bottom > output.height
                || (right != output.width && right % bw != 0)
                || (bottom != output.height && bottom % bh != 0)
            {
                return Err(DeviceError::InvalidCall);
            }
            copies.push(CopyRect {
                source,
                destination: point,
            });
        }
        Ok(copies)
    }
}
