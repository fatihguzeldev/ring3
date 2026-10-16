use super::super::{
    commands::{Command, DrawBindings, PatchDraw, PatchSource, PipelineId},
    patch::{
        PatchInfo, Segments,
        domain::{DomainKey, DomainMesh, EdgeStyle},
        program::{InputPlan, SurfaceProgram},
    },
    shader::{Fvf, registry::VertexShader},
};
use super::{Device, DeviceError, Outcome};
use std::sync::Arc;

pub(super) struct CachedPatch {
    handle: u32,
    pub(super) identity: u64,
    program: SurfaceProgram,
    vertex_shader: u32,
    bytes: u32,
    _vertex: Option<Arc<VertexShader>>,
    _fvf: Option<Arc<Fvf>>,
}

impl Device {
    pub fn draw_patch(
        &mut self,
        handle: u32,
        triangular: bool,
        info: Option<PatchInfo>,
        segments: Option<Segments>,
        mut bindings: DrawBindings,
    ) -> Result<Outcome<()>, DeviceError> {
        self.available()?;
        if !self.scene {
            return Err(DeviceError::InvalidCall);
        }
        let previous = self.patches.iter().position(|patch| patch.handle == handle);
        let shader = self.state.snapshot().vertex_shader;
        let program = if let Some(info) = info {
            if info.triangular() != triangular {
                return Err(DeviceError::InvalidCall);
            }
            let (first, count) = info.vertex_extent().map_err(|_| DeviceError::InvalidCall)?;
            self.validate_bindings(&bindings)?;
            self.validate_vertex_range(&bindings, first, count)?;
            SurfaceProgram::compile(info).map_err(|_| DeviceError::InvalidCall)?
        } else {
            let patch = &self.patches[previous.ok_or(DeviceError::InvalidCall)?];
            if handle == 0
                || patch.program.triangular() != triangular
                || patch.vertex_shader != shader
            {
                return Err(DeviceError::InvalidCall);
            }
            bindings.streams.fill(None);
            bindings.indices = None;
            self.validate_bindings(&bindings)?;
            patch.program
        };
        let segments = match segments {
            Some(segments) => segments,
            None => Segments::uniform(self.state.snapshot().render[164], triangular)
                .map_err(|_| DeviceError::InvalidCall)?,
        };
        segments
            .validate(triangular)
            .map_err(|_| DeviceError::InvalidCall)?;
        let key = DomainKey {
            triangular,
            segments,
            style: EdgeStyle::try_from(self.state.snapshot().render[163])
                .map_err(|_| DeviceError::InvalidCall)?,
        };
        let input_plan = if let Some(vertex) = &self.vertex {
            InputPlan::compile(
                vertex.declaration.elements(),
                vertex.declaration.tessellation(),
            )
        } else if let Some(fvf) = &self.fvf {
            InputPlan::compile(fvf.elements(), &[]).and_then(|plan| {
                if fvf.blend_weights == 5 {
                    plan.with_fifth_blend_weight(fvf.weight_offsets[4].unwrap())
                } else {
                    Ok(plan)
                }
            })
        } else {
            return Err(DeviceError::InvalidCall);
        }
        .map_err(|_| DeviceError::InvalidCall)?;
        let bytes = program
            .control_count()
            .checked_mul(17 * 16)
            .ok_or(DeviceError::Capacity)?;
        let retained = if handle != 0 && info.is_some() {
            self.patches
                .iter()
                .enumerate()
                .filter(|(i, _)| Some(*i) != previous)
                .map(|(_, patch)| u64::from(patch.bytes))
                .sum::<u64>()
        } else {
            0
        };
        if u64::from(bytes) > u64::from(self.limits.graphics.resource_bytes)
            || (handle != 0
                && info.is_some()
                && retained + u64::from(bytes) > u64::from(self.limits.graphics.resource_bytes))
        {
            return Err(DeviceError::Capacity);
        }
        self.validate_device()?;
        if handle != 0
            && info.is_some()
            && previous.is_none()
            && self.patches.len() == self.limits.patches as usize
        {
            return Err(DeviceError::Capacity);
        }
        let next_identity = if handle != 0 && info.is_some() {
            self.patch_sequence
                .checked_add(1)
                .ok_or(DeviceError::IdentityExhausted)?
        } else {
            0
        };
        let changed_domain = self.patch_domain.is_none_or(|(old, _)| old != key);
        let domain = if changed_domain {
            self.patch_domain_sequence
                .checked_add(1)
                .ok_or(DeviceError::IdentityExhausted)?
        } else {
            self.patch_domain.unwrap().1
        };
        if changed_domain {
            if self.patch_workspace.is_none() {
                self.patch_workspace =
                    Some(DomainMesh::new().map_err(|_| DeviceError::Allocation)?);
            }
            self.patch_workspace
                .as_mut()
                .unwrap()
                .rebuild(key)
                .map_err(|_| DeviceError::InvalidCall)?;
        }
        self.drain_retired()?;
        let targets = self.output_targets(true, self.depth_target.is_some())?;
        let revision = self.state.revision();
        let (pipeline, changed) = match self.emitted_pipeline {
            Some((old, id)) if old == revision => (id, false),
            _ => (
                PipelineId(
                    self.pipeline_sequence
                        .checked_add(1)
                        .ok_or(DeviceError::IdentityExhausted)?,
                ),
                true,
            ),
        };
        let streams = bindings.streams;
        let source = if info.is_some() && handle == 0 {
            PatchSource::Inline {
                program: &program,
                inputs: &input_plan,
                streams: &streams,
            }
        } else {
            PatchSource::Cached {
                identity: if next_identity != 0 {
                    next_identity
                } else {
                    self.patches[previous.unwrap()].identity
                },
            }
        };
        bindings.streams.fill(None);
        bindings.indices = None;
        let patch_draw = PatchDraw {
            pipeline,
            domain,
            bindings,
            targets,
            source,
        };
        let draw = Command::DrawPatch(&patch_draw);
        let progress = if changed || changed_domain || next_identity != 0 || !self.patch_kernel {
            let mut commands = Vec::new();
            commands
                .try_reserve_exact(8)
                .map_err(|_| DeviceError::Allocation)?;
            if !self.patch_kernel {
                commands.push(Command::PatchKernel);
            }
            if changed {
                commands.push(Command::Pipeline {
                    id: pipeline,
                    state: self.state.snapshot(),
                });
            }
            if next_identity != 0 {
                commands.push(Command::DefinePatch {
                    identity: next_identity,
                    vertex_shader: shader,
                    program: &program,
                    inputs: &input_plan,
                    streams: &streams,
                });
            }
            if changed_domain {
                commands.push(Command::DefinePatchDomain {
                    identity: domain,
                    mesh: self.patch_workspace.as_ref().unwrap(),
                });
            }
            commands.push(draw);
            if next_identity != 0
                && let Some(index) = previous
            {
                commands.push(Command::DeletePatch {
                    identity: self.patches[index].identity,
                });
            }
            if changed_domain && let Some((_, identity)) = self.patch_domain {
                commands.push(Command::DeletePatchDomain { identity });
            }
            let prepared = self.queue.prepare(&commands)?;
            self.queue.commit(prepared)?
        } else {
            self.queue.submit(&draw)?
        };
        self.commit_target_writes(true, self.depth_target.is_some())?;
        self.patch_kernel = true;
        if changed {
            self.pipeline_sequence = pipeline.0;
            self.emitted_pipeline = Some((revision, pipeline));
        }
        if changed_domain {
            self.patch_domain = Some((key, domain));
            self.patch_domain_sequence = domain;
        }
        if next_identity != 0 {
            let patch = CachedPatch {
                handle,
                identity: next_identity,
                program,
                vertex_shader: shader,
                bytes,
                _vertex: self.vertex.clone(),
                _fvf: self.fvf.clone(),
            };
            if let Some(index) = previous {
                self.patches[index] = patch;
            } else {
                self.patches.push(patch);
            }
            self.patch_sequence = next_identity;
        }
        Ok(Outcome::new((), progress))
    }

    pub fn delete_patch(&mut self, handle: u32) -> Result<Outcome<()>, DeviceError> {
        self.available()?;
        let index = self
            .patches
            .iter()
            .position(|patch| patch.handle == handle)
            .ok_or(DeviceError::InvalidCall)?;
        let progress = self.queue.submit(&Command::DeletePatch {
            identity: self.patches[index].identity,
        })?;
        self.patches.swap_remove(index);
        Ok(Outcome::new((), progress))
    }
}
