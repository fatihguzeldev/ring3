use super::save::TaskSaveResult;
use crate::{
    abi::{
        arena::STATE_OFFSET,
        x86::{
            SEGMENT_BASES_RECORD_SIZE, SEGMENTS_SIZE, STATE_SIZE, StateMetadata,
            USER_FLAGS_MASK_V3, encode_current_segment_context, encode_state_current,
        },
    },
    cpu::x86::{
        SegmentRegister,
        tasks::{
            TaskAuthorityRequirement32, TaskControlState32, TaskIncomingContext32, TaskLoadState32,
            TaskRecord, TaskReturnAdmission, TaskSave, TaskSaveOverlayAuthority, TaskSavePlan,
            TaskTransitionFault32,
        },
    },
    memory::{GuestAddress, MemoryError, MemoryFault},
    process::{EngineInstance, HostError, StoreCompletion},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommittedTaskReturn32 {
    pub context: TaskIncomingContext32,
    pub controls: TaskControlState32,
    pub currentness: Result<StoreCompletion, HostError>,
    /// false requires the pending raw task-context representation, not ordinary r3sg decoding.
    pub normal_segments_published: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskReturnTransition32 {
    PreCommitFault(TaskTransitionFault32),
    NeedsAuthority(TaskAuthorityRequirement32),
    NeedsPagingAuthority { fault: MemoryFault, length: u32 },
    Infrastructure(MemoryError),
    Committed(CommittedTaskReturn32),
}

impl EngineInstance {
    pub fn task_return_transition(
        &mut self,
        outgoing: TaskSave,
    ) -> Result<TaskReturnTransition32, HostError> {
        self.guard_material(self.key, self.generation())?;
        self.task_return_transition_bound(None, outgoing)
    }

    pub fn task_return_transition_resident(
        &mut self,
        key: u64,
        id: u64,
        outgoing: TaskSave,
    ) -> Result<TaskReturnTransition32, HostError> {
        self.guard_material_resident(key, id)?;
        self.task_return_transition_bound(Some(id), outgoing)
    }

    fn task_return_transition_bound(
        &mut self,
        resident: Option<u64>,
        mut outgoing: TaskSave,
    ) -> Result<TaskReturnTransition32, HostError> {
        if outgoing.flags & 2 == 0
            || outgoing.flags & 0x4000 == 0
            || outgoing.flags & !USER_FLAGS_MASK_V3 != 0
            || outgoing.selectors != self.operand_segment_context()?.state.selectors
        {
            return Err(HostError::InvalidRequest);
        }
        let admission = match resident {
            Some(id) => self.prepare_task_return_resident(self.key, id)?,
            None => self.prepare_task_return()?,
        };
        let prepared = match admission {
            TaskReturnAdmission::Admitted(prepared) => prepared,
            TaskReturnAdmission::InvalidTss(code) => {
                return Ok(self.task_precommit_fault(TaskTransitionFault32::InvalidTss(code)));
            }
            TaskReturnAdmission::NotPresent(code) => {
                return Ok(self.task_precommit_fault(TaskTransitionFault32::NotPresent(code)));
            }
            TaskReturnAdmission::MissingAuthority => {
                return Ok(TaskReturnTransition32::NeedsAuthority(
                    TaskAuthorityRequirement32::Sources,
                ));
            }
            TaskReturnAdmission::NeedsPagingAuthority { fault, length } => {
                return Ok(TaskReturnTransition32::NeedsPagingAuthority { fault, length });
            }
            TaskReturnAdmission::Infrastructure(error) => {
                return Ok(TaskReturnTransition32::Infrastructure(error));
            }
        };
        let Some(authority) = self.task_controls else {
            return Ok(TaskReturnTransition32::NeedsAuthority(
                TaskAuthorityRequirement32::Controls,
            ));
        };
        if authority.memory_identity != self.memory()?.identity()
            || authority.state.paging_enabled()
        {
            return Ok(TaskReturnTransition32::NeedsAuthority(
                TaskAuthorityRequirement32::AddressSpace,
            ));
        }
        outgoing.flags &= !0x10000;
        let plan = TaskSavePlan::new(prepared.current().format(), &outgoing);
        if prepared.current().limit() < plan.required_limit() {
            return Ok(
                self.task_precommit_fault(TaskTransitionFault32::InvalidTss(u32::from(
                    prepared.current().selector() & !3,
                ))),
            );
        }
        let Some(old_record) = self
            .segment_descriptors
            .raw_descriptor(prepared.current().selector())
            .copied()
        else {
            return Ok(TaskReturnTransition32::NeedsAuthority(
                TaskAuthorityRequirement32::Descriptor(prepared.current().selector()),
            ));
        };
        let format = prepared.target().format();
        let length = format.record_bytes();
        let mut storage = [0; 104];
        let bytes = &mut storage[..length];
        if let Err(error) = self
            .memory()?
            .cpu_read(GuestAddress(prepared.target().base()), bytes)
        {
            return Ok(match error {
                MemoryError::Fault(fault) => TaskReturnTransition32::NeedsPagingAuthority {
                    fault,
                    length: length as u32,
                },
                error => TaskReturnTransition32::Infrastructure(error),
            });
        }
        // the current concrete address space has unique linear mappings; no hidden physical aliases are inferred.
        plan.overlay(
            prepared.current().base(),
            prepared.target().base(),
            bytes,
            TaskSaveOverlayAuthority::SameLinearAddressSpace,
        )
        .expect("bounded known-linear task image");
        let incoming = TaskLoadState32::from_record(
            TaskRecord::new(format, bytes)
                .expect("complete task image")
                .decode(),
        );
        let sources = self.task_return_sources.expect("admitted task sources");
        let context = match crate::cpu::x86::tasks::incoming_segments(
            incoming,
            &self.segment_descriptors,
            sources.gdt_limit,
            (
                prepared.current().selector(),
                old_record.with_task_busy_cleared(),
            ),
        ) {
            Ok(context) => context,
            Err(requirement) => return Ok(TaskReturnTransition32::NeedsAuthority(requirement)),
        };
        let next_revision = self
            .task_execution_revision
            .checked_add(1)
            .ok_or(HostError::GenerationExhausted)?;
        let mut state = [0; STATE_SIZE];
        encode_state_current(&context.state(), &mut state)
            .map_err(|_| HostError::Infrastructure)?;
        StateMetadata {
            ss_debug_shadow: false,
        }
        .write(&mut state);
        let mut segment_bytes = [0; SEGMENTS_SIZE];
        let mut bases = [0; SEGMENT_BASES_RECORD_SIZE];
        let normal_segments = context.validated_segments();
        if let Some(segments) = normal_segments {
            encode_current_segment_context(&segments, &mut segment_bytes, &mut bases)
                .map_err(|_| HostError::Infrastructure)?;
        }
        let saved = match resident {
            Some(id) => self.save_outgoing_task_resident(self.key, id, &outgoing)?,
            None => self.save_outgoing_task(&outgoing)?,
        };
        let currentness = match saved {
            TaskSaveResult::Saved { currentness } => currentness,
            TaskSaveResult::InvalidTss(code) => {
                return Ok(self.task_precommit_fault(TaskTransitionFault32::InvalidTss(code)));
            }
            TaskSaveResult::MissingAuthority => {
                return Ok(TaskReturnTransition32::NeedsAuthority(
                    TaskAuthorityRequirement32::Sources,
                ));
            }
            TaskSaveResult::NeedsPagingAuthority { fault, length } => {
                return Ok(TaskReturnTransition32::NeedsPagingAuthority { fault, length });
            }
            TaskSaveResult::Infrastructure(error) => {
                return Ok(TaskReturnTransition32::Infrastructure(error));
            }
        };
        // ram publication is the barrier: only infallible in-place task/state updates follow.
        self.segment_descriptors
            .commit_task_busy_clear(prepared.current().selector());
        for register in [
            SegmentRegister::Es,
            SegmentRegister::Cs,
            SegmentRegister::Ss,
            SegmentRegister::Ds,
            SegmentRegister::Fs,
            SegmentRegister::Gs,
        ] {
            if context.loaded_mask() & (1 << register.index()) != 0 {
                self.segment_descriptors
                    .commit_accessed(context.selectors()[register.index()]);
            }
        }
        self.task_return_sources.as_mut().unwrap().current = prepared.target();
        let controls = authority
            .state
            .after_return(context.ldtr(), context.current_privilege());
        self.task_controls.as_mut().unwrap().state = controls;
        self.task_execution_revision = next_revision;
        self.task_incoming = Some(context);
        self.arena.as_mut().get_mut()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE]
            .copy_from_slice(&state);
        if normal_segments.is_some() {
            self.publish_segment_context(&segment_bytes, &bases);
        }
        Ok(TaskReturnTransition32::Committed(CommittedTaskReturn32 {
            context,
            controls,
            currentness,
            normal_segments_published: normal_segments.is_some(),
        }))
    }

    fn task_precommit_fault(&mut self, fault: TaskTransitionFault32) -> TaskReturnTransition32 {
        if let Some(controls) = self.task_controls.as_mut() {
            controls.state.unblock_nmi();
        }
        TaskReturnTransition32::PreCommitFault(fault)
    }
}

#[cfg(test)]
#[path = "transition/tests.rs"]
mod tests;
