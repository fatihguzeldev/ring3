use super::super::{
    commands::{Command, Draw, DrawBindings, DrawData, PipelineId, Primitive, Progress, Targets},
    resources::{BufferKind, Format, ResourceDesc, SurfaceRef},
    shader::Element,
};
use super::{Device, DeviceError, Outcome};

impl Device {
    pub fn begin_scene(&mut self) -> Result<(), DeviceError> {
        self.available()?;
        if self.scene {
            return Err(DeviceError::InvalidCall);
        }
        self.scene = true;
        Ok(())
    }

    pub fn end_scene(&mut self) -> Result<(), DeviceError> {
        self.available()?;
        if !self.scene {
            return Err(DeviceError::InvalidCall);
        }
        self.scene = false;
        Ok(())
    }

    pub fn draw(
        &mut self,
        primitive: Primitive,
        primitive_count: u32,
        bindings: DrawBindings,
        data: DrawData<'_>,
    ) -> Result<Outcome<()>, DeviceError> {
        self.available()?;
        if !self.scene {
            return Err(DeviceError::InvalidCall);
        }
        let count = primitive
            .element_count(primitive_count)
            .ok_or(DeviceError::InvalidCall)?;
        if count == 0 {
            if matches!(
                data,
                DrawData::UserVertices { .. } | DrawData::UserIndexed { .. }
            ) {
                self.state
                    .finish_user_draw(matches!(data, DrawData::UserIndexed { .. }))?;
            }
            return Ok(Outcome::new((), Progress::Queued));
        }
        self.validate_bindings(&bindings)?;
        self.validate_draw(&bindings, &data, count)?;
        self.validate_device()?;
        let user = matches!(
            data,
            DrawData::UserVertices { .. } | DrawData::UserIndexed { .. }
        );
        let user_indexed = matches!(data, DrawData::UserIndexed { .. });
        if user
            && self.state.user_draw_changes_bindings(user_indexed)
            && self.state.revision() == u64::MAX
        {
            return Err(DeviceError::IdentityExhausted);
        }
        self.drain_retired()?;
        let targets = self.output_targets(true, self.depth_target.is_some())?;
        let revision = self.state.revision();
        let (pipeline, changed) = match self.emitted_pipeline {
            Some((previous, id)) if previous == revision => (id, false),
            _ => (
                PipelineId(
                    self.pipeline_sequence
                        .checked_add(1)
                        .ok_or(DeviceError::IdentityExhausted)?,
                ),
                true,
            ),
        };
        let draw = Command::Draw(Draw {
            pipeline,
            primitive,
            primitive_count,
            bindings,
            targets,
            data,
        });
        let progress = if changed {
            let prepared = self.queue.prepare(&[
                Command::Pipeline {
                    id: pipeline,
                    state: self.state.snapshot(),
                },
                draw,
            ])?;
            self.queue.commit(prepared)?
        } else {
            self.queue.submit(&draw)?
        };
        self.commit_target_writes(true, self.depth_target.is_some())?;
        if changed {
            self.pipeline_sequence = pipeline.0;
            self.emitted_pipeline = Some((revision, pipeline));
        }
        if user {
            self.state.finish_user_draw(user_indexed)?;
        }
        Ok(Outcome::new((), progress))
    }

    pub(super) fn validate_bindings(&self, bindings: &DrawBindings) -> Result<(), DeviceError> {
        for (index, binding) in bindings.textures.iter().enumerate() {
            if binding.is_some() != (self.state.snapshot().textures[index] != 0) {
                return Err(DeviceError::InvalidCall);
            }
            if let Some(binding) = binding {
                if self.resources.version(binding.resource.id)? != binding.resource.version
                    || !matches!(
                        self.resources.desc(binding.resource.id)?,
                        ResourceDesc::Texture(_)
                    )
                    || self.resources.lod(binding.resource.id)? != binding.lod
                    || self.resources.is_locked(binding.resource.id)?
                    || binding.resource.id == self.color_target.resource
                {
                    return Err(DeviceError::InvalidCall);
                }
                self.ensure_resource_idle(binding.resource.id)?;
            }
        }
        for binding in bindings.streams.iter().flatten() {
            if self.resources.version(binding.resource.id)? != binding.resource.version
                || !matches!(self.resources.desc(binding.resource.id)?, ResourceDesc::Buffer(desc) if matches!(desc.kind, BufferKind::Vertex { .. }))
                || self.resources.is_locked(binding.resource.id)?
            {
                return Err(DeviceError::InvalidCall);
            }
            self.ensure_resource_idle(binding.resource.id)?;
        }
        if let Some(binding) = bindings.indices {
            if self.resources.version(binding.resource.id)? != binding.resource.version
                || !matches!(self.resources.desc(binding.resource.id)?, ResourceDesc::Buffer(desc) if desc.kind == BufferKind::Index { format: binding.format })
                || self.resources.is_locked(binding.resource.id)?
            {
                return Err(DeviceError::InvalidCall);
            }
            self.ensure_resource_idle(binding.resource.id)?;
        }
        Ok(())
    }

    pub(super) fn elements(&self) -> Result<&[Element], DeviceError> {
        if let Some(shader) = &self.vertex {
            return Ok(shader.declaration.elements());
        }
        self.fvf
            .as_ref()
            .map(|fvf| fvf.elements())
            .ok_or(DeviceError::InvalidCall)
    }

    pub(super) fn validate_vertex_range(
        &self,
        bindings: &DrawBindings,
        first: u32,
        count: u32,
    ) -> Result<(), DeviceError> {
        let last = first
            .checked_add(count)
            .and_then(|end| end.checked_sub(1))
            .ok_or(DeviceError::InvalidCall)?;
        for element in self.elements()? {
            let binding =
                bindings.streams[element.stream as usize].ok_or(DeviceError::InvalidCall)?;
            let ResourceDesc::Buffer(desc) = self.resources.desc(binding.resource.id)? else {
                return Err(DeviceError::InvalidCall);
            };
            let element_end = element
                .offset
                .checked_add(element.kind.bytes())
                .ok_or(DeviceError::InvalidCall)?;
            if (binding.stride != 0 && binding.stride < element_end)
                || binding.stride > 2048
                || last
                    .checked_mul(binding.stride)
                    .and_then(|start| start.checked_add(element_end))
                    .is_none_or(|end| end > desc.length)
            {
                return Err(DeviceError::InvalidCall);
            }
        }
        Ok(())
    }

    fn validate_draw(
        &self,
        bindings: &DrawBindings,
        data: &DrawData<'_>,
        count: u32,
    ) -> Result<(), DeviceError> {
        match *data {
            DrawData::Vertices { first_vertex } => {
                self.validate_vertex_range(bindings, first_vertex, count)
            }
            DrawData::Indexed {
                min_vertex,
                vertex_count,
                start_index,
            } => {
                if vertex_count == 0 {
                    return Err(DeviceError::InvalidCall);
                }
                let indices = bindings.indices.ok_or(DeviceError::InvalidCall)?;
                let bytes = self.resources.bytes(SurfaceRef {
                    resource: indices.resource.id,
                    subresource: 0,
                })?;
                let stride = index_stride(indices.format)?;
                let first = start_index
                    .checked_mul(stride)
                    .ok_or(DeviceError::InvalidCall)? as usize;
                let end = count
                    .checked_mul(stride)
                    .and_then(|length| first.checked_add(length as usize))
                    .ok_or(DeviceError::InvalidCall)?;
                validate_indices(
                    bytes.get(first..end).ok_or(DeviceError::InvalidCall)?,
                    indices.format,
                    min_vertex,
                    vertex_count,
                )?;
                let first = indices
                    .base_vertex
                    .checked_add(min_vertex)
                    .ok_or(DeviceError::InvalidCall)?;
                self.validate_vertex_range(bindings, first, vertex_count)
            }
            DrawData::UserVertices { stride, vertices } => {
                self.validate_user_vertices(stride, vertices, 0, count)
            }
            DrawData::UserIndexed {
                min_vertex,
                vertex_count,
                stride,
                vertices,
                format,
                indices,
            } => {
                if vertex_count == 0
                    || indices.len()
                        != count
                            .checked_mul(index_stride(format)?)
                            .ok_or(DeviceError::InvalidCall)? as usize
                {
                    return Err(DeviceError::InvalidCall);
                }
                validate_indices(indices, format, min_vertex, vertex_count)?;
                self.validate_user_vertices(stride, vertices, min_vertex, vertex_count)
            }
        }
    }

    fn validate_user_vertices(
        &self,
        stride: u32,
        vertices: &[u8],
        first: u32,
        count: u32,
    ) -> Result<(), DeviceError> {
        if stride == 0
            || stride > 2048
            || first.checked_add(count).is_none()
            || count
                .checked_mul(stride)
                .is_none_or(|length| length as usize != vertices.len())
        {
            return Err(DeviceError::InvalidCall);
        }
        for element in self.elements()? {
            if element.stream != 0
                || element
                    .offset
                    .checked_add(element.kind.bytes())
                    .is_none_or(|end| end > stride)
            {
                return Err(DeviceError::InvalidCall);
            }
        }
        Ok(())
    }

    pub(super) fn output_targets(
        &self,
        color_write: bool,
        depth_write: bool,
    ) -> Result<Targets, DeviceError> {
        self.ensure_resource_idle(self.color_target.resource)?;
        let mut color = self.surface_version(self.color_target)?;
        if color_write {
            self.resources.can_mark_gpu_written(color.surface)?;
            color.version = color
                .version
                .checked_add(1)
                .ok_or(DeviceError::IdentityExhausted)?;
        }
        let depth = if let Some(surface) = self.depth_target {
            self.ensure_resource_idle(surface.resource)?;
            let mut depth = self.surface_version(surface)?;
            if depth_write {
                self.resources.can_mark_gpu_written(surface)?;
                depth.version = depth
                    .version
                    .checked_add(1)
                    .ok_or(DeviceError::IdentityExhausted)?;
            }
            Some(depth)
        } else {
            None
        };
        Ok(Targets { color, depth })
    }

    pub(super) fn commit_target_writes(
        &mut self,
        color: bool,
        depth: bool,
    ) -> Result<(), DeviceError> {
        if color {
            self.resources.mark_gpu_written(self.color_target)?;
        }
        if depth && let Some(surface) = self.depth_target {
            self.resources.mark_gpu_written(surface)?;
        }
        Ok(())
    }
}

fn index_stride(format: Format) -> Result<u32, DeviceError> {
    match format {
        Format::Index16 => Ok(2),
        Format::Index32 => Ok(4),
        _ => Err(DeviceError::InvalidCall),
    }
}

fn validate_indices(
    bytes: &[u8],
    format: Format,
    minimum: u32,
    count: u32,
) -> Result<(), DeviceError> {
    let end = minimum.checked_add(count).ok_or(DeviceError::InvalidCall)?;
    for bytes in bytes.chunks_exact(index_stride(format)? as usize) {
        let index = if format == Format::Index16 {
            u32::from(u16::from_le_bytes(bytes.try_into().unwrap()))
        } else {
            u32::from_le_bytes(bytes.try_into().unwrap())
        };
        if index < minimum || index >= end {
            return Err(DeviceError::InvalidCall);
        }
    }
    Ok(())
}
