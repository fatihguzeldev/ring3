use crate::{
    abi::{
        arena::{EXIT_OFFSET, STATE_OFFSET, WORK_OFFSET, WORK_SIZE},
        header,
        x86::{EXIT_SIZE, STATE_SIZE, decode_exit, decode_state, decode_state_metadata},
    },
    cpu::{
        ExecutionExit, ExitReason,
        x86::{EventOrigin32, ExceptionCause32, FrameInput32, Register32, SegmentRegister},
    },
    memory::MemoryFault,
    process::{
        CallError, EngineInstance, HostError,
        calls::{call::PendingOwner, context::ExtendedCpuState},
    },
};

mod restart;
pub(in crate::process) mod site;
pub(in crate::process) use restart::InstructionTrapRestart32;

/// detector-supplied provenance; software page permissions do not certify an architectural #PF.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::process) enum DetectedEvent32 {
    Exception(EventOrigin32),
    NeedsPagingAuthority { fault: MemoryFault, length: u32 },
}

// the owner boxes this slot once; keeping ready inline avoids an allocation at every fault.
#[allow(clippy::large_enum_variant)]
pub(in crate::process) enum DetectionSlot32 {
    Idle,
    Shutdown,
    Active {
        epoch: u64,
        owner: PendingOwner,
        callback_token: Option<u32>,
    },
    Ready(ConsumedDetection32),
}

/// one opaque sealed snapshot, movable only by consume; no duplicate compiled-material ownership.
pub(in crate::process) struct ConsumedDetection32 {
    epoch: u64,
    owner: PendingOwner,
    callback_token: Option<u32>,
    event: DetectedEvent32,
    input: FrameInput32,
    restart: Option<InstructionTrapRestart32>,
    state: [u8; STATE_SIZE],
    exit: [u8; EXIT_SIZE],
    extended: ExtendedCpuState,
    work: [u8; WORK_SIZE],
    retired: u32,
    task_context: Option<crate::cpu::x86::tasks::TaskIncomingContext32>,
    task_revision: Option<u64>,
}

impl ConsumedDetection32 {
    #[cfg(test)]
    pub(in crate::process) const fn epoch(&self) -> u64 {
        self.epoch
    }
    pub(in crate::process) const fn owner(&self) -> PendingOwner {
        self.owner
    }
    pub(in crate::process) const fn event(&self) -> DetectedEvent32 {
        self.event
    }
    pub(in crate::process) const fn input(&self) -> FrameInput32 {
        self.input
    }
    pub(in crate::process) const fn instruction_restart(&self) -> Option<InstructionTrapRestart32> {
        self.restart
    }
    pub(in crate::process) const fn retired(&self) -> u32 {
        self.retired
    }
    pub(in crate::process) fn state(&self) -> &[u8; STATE_SIZE] {
        &self.state
    }
    pub(in crate::process) fn exit(&self) -> &[u8; EXIT_SIZE] {
        &self.exit
    }
    fn matches(&self, arena: &[u8]) -> bool {
        arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE] == self.state
            && arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE] == self.exit
            && self.extended.matches(arena)
            && arena[WORK_OFFSET..WORK_OFFSET + WORK_SIZE] == self.work
    }
}

impl DetectedEvent32 {
    /// consistency only: the origin was already selected at the trusted detector site.
    /// this function never manufactures a cause from a host-editable R3EX record.
    fn matches_exit(self, exit: ExecutionExit) -> bool {
        match (self, exit.reason) {
            (
                Self::NeedsPagingAuthority { fault, length },
                ExitReason::MemoryFault {
                    fault: actual,
                    length: actual_length,
                },
            ) => fault == actual && length == actual_length,
            (Self::Exception(EventOrigin32::Exception(cause)), reason) => match (cause, reason) {
                (ExceptionCause32::DivideError, ExitReason::DivideError)
                | (ExceptionCause32::BoundRange, ExitReason::BoundRange)
                | (ExceptionCause32::InvalidOpcode, ExitReason::InvalidOpcode)
                | (ExceptionCause32::DeviceNotAvailable, ExitReason::DeviceNotAvailable)
                | (ExceptionCause32::BreakpointInt3, ExitReason::Breakpoint)
                | (ExceptionCause32::OverflowInto, ExitReason::Overflow)
                | (ExceptionCause32::Debug(_), ExitReason::DebugTrap) => true,
                (ExceptionCause32::InvalidTss(code), ExitReason::InvalidTss { error_code })
                | (
                    ExceptionCause32::GeneralProtection(code),
                    ExitReason::GeneralProtection { error_code },
                )
                | (
                    ExceptionCause32::SegmentNotPresent(code),
                    ExitReason::SegmentNotPresent { error_code },
                )
                | (ExceptionCause32::StackSegment(code), ExitReason::StackSegment { error_code }) => {
                    code.get() == error_code
                }
                (
                    ExceptionCause32::FloatingPoint { status },
                    ExitReason::FloatingPoint { status: actual },
                ) => status == actual,
                (
                    ExceptionCause32::SimdFloatingPoint { mxcsr },
                    ExitReason::SimdFloatingPoint { mxcsr: actual },
                ) => mxcsr == actual,
                // verified PF still requires a separate detection/CR2 owner; no R3EX PF inference.
                _ => false,
            },
            _ => false,
        }
    }
}

impl EngineInstance {
    /// root's bound entry guard has already validated current material and decode interpretation.
    /// an unfinished lease is the explicit non-event-return cleanup on the next valid entry.
    pub(in crate::process) fn begin_exception_detection(
        &mut self,
        owner: PendingOwner,
    ) -> Result<u64, HostError> {
        self.memory()?;
        if self.is_exception_shutdown() {
            return Err(HostError::CpuShutdown);
        }
        if self.interrupt_descriptors.is_none() {
            return Err(HostError::InvalidRequest);
        }
        if self.has_ready_exception_detection() {
            return Err(HostError::Call(CallError::Busy));
        }
        let callback_token = self.callback.as_ref().map(|callback| callback.token());
        self.check_detection_owner(owner, callback_token)?;
        let epoch = self
            .detection_epoch
            .checked_add(1)
            .filter(|value| *value <= i64::MAX as u64)
            .ok_or(HostError::GenerationExhausted)?;
        let active = DetectionSlot32::Active {
            epoch,
            owner,
            callback_token,
        };
        if let Some(slot) = self.exception_detection.as_mut() {
            slot[0] = active;
        } else {
            let mut slot = Vec::new();
            slot.try_reserve_exact(1)
                .map_err(|_| HostError::Infrastructure)?;
            slot.push(active);
            let slot: Box<[DetectionSlot32; 1]> = slot
                .into_boxed_slice()
                .try_into()
                .map_err(|_| HostError::Infrastructure)?;
            self.exception_detection = Some(slot);
        }
        self.detection_epoch = epoch;
        Ok(epoch)
    }

    /// called once after all local GPR/flags/EIP, exit and REP work outputs are flushed.
    /// entry lease authorizes stale physical code; no currentness guard is repeated here.
    pub(in crate::process) fn finish_exception_detection(
        &mut self,
        epoch: u64,
        event: DetectedEvent32,
        restart: Option<InstructionTrapRestart32>,
    ) -> Result<(), HostError> {
        self.memory()?;
        let (owner, callback_token) = match self.exception_detection.as_ref().map(|slot| &slot[0]) {
            Some(DetectionSlot32::Active {
                epoch: active,
                owner,
                callback_token,
            }) if *active == epoch => (*owner, *callback_token),
            _ => return Err(HostError::Call(CallError::InvalidToken)),
        };
        self.check_detection_owner(owner, callback_token)?;
        let arena = self.arena();
        let state_bytes: [u8; STATE_SIZE] = arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE]
            .try_into()
            .unwrap();
        let exit_bytes: [u8; EXIT_SIZE] = arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE]
            .try_into()
            .unwrap();
        let state =
            decode_state(&state_bytes).map_err(|_| HostError::Call(CallError::InvalidStop))?;
        let exit = decode_exit(&exit_bytes).map_err(|_| HostError::Call(CallError::InvalidStop))?;
        if !event.matches_exit(exit) {
            return Err(HostError::Call(CallError::InvalidStop));
        }
        let extended = ExtendedCpuState::capture(arena)?;
        let work: [u8; WORK_SIZE] = arena[WORK_OFFSET..WORK_OFFSET + WORK_SIZE]
            .try_into()
            .unwrap();
        header::validate(&work, *b"R3WK", WORK_SIZE)
            .map_err(|_| HostError::Call(CallError::InvalidStop))?;
        if header::read_u32(&work, 20) != 0 {
            return Err(HostError::Call(CallError::InvalidStop));
        }
        let context = match self.task_context_snapshot()? {
            Some(context) => context.segment_image(),
            None => self.segment_context()?,
        };
        let input = FrameInput32 {
            eip: state.eip,
            esp: state.registers[Register32::Esp.index()],
            flags: state.eflags,
            cs: context.state.selectors[SegmentRegister::Cs.index()],
        };
        let metadata = decode_state_metadata(&state_bytes)
            .map_err(|_| HostError::Call(CallError::InvalidStop))?;
        if !restart::valid_provenance(
            event,
            restart,
            input,
            exit.retired,
            metadata.ss_debug_shadow,
            header::read_u32(&work, 16),
        ) {
            return Err(HostError::Call(CallError::InvalidStop));
        }
        let record = ConsumedDetection32 {
            epoch,
            owner,
            callback_token,
            event,
            input,
            restart,
            state: state_bytes,
            exit: exit_bytes,
            extended,
            work,
            retired: exit.retired,
            task_context: self.task_incoming,
            task_revision: self.task_execution_revision(),
        };
        self.exception_detection.as_mut().unwrap()[0] = DetectionSlot32::Ready(record);
        Ok(())
    }

    pub(in crate::process) fn consume_exception_detection(
        &mut self,
        epoch: u64,
    ) -> Result<ConsumedDetection32, HostError> {
        self.memory()?;
        let record = match self.exception_detection.as_ref().map(|slot| &slot[0]) {
            Some(DetectionSlot32::Ready(record)) if record.epoch == epoch => record,
            _ => return Err(HostError::Call(CallError::InvalidToken)),
        };
        self.check_detection_owner(record.owner, record.callback_token)?;
        if !record.matches(self.arena())
            || record.task_context != self.task_incoming
            || record.task_revision != self.task_execution_revision()
        {
            return Err(HostError::Call(CallError::StateChanged));
        }
        let DetectionSlot32::Ready(record) = std::mem::replace(
            &mut self.exception_detection.as_mut().unwrap()[0],
            DetectionSlot32::Idle,
        ) else {
            unreachable!("ready record checked before infallible single consumption")
        };
        Ok(record)
    }

    /// test-only abandonment; production consumes ready atomically or replaces an active non-event lease.
    #[cfg(test)]
    pub(in crate::process) fn discard_exception_detection(
        &mut self,
        epoch: u64,
    ) -> Result<(), HostError> {
        self.memory()?;
        if self.is_exception_shutdown() {
            return Err(HostError::CpuShutdown);
        }
        let matches = match self.exception_detection.as_ref().map(|slot| &slot[0]) {
            Some(DetectionSlot32::Active { epoch: active, .. }) => *active == epoch,
            Some(DetectionSlot32::Ready(record)) => record.epoch == epoch,
            _ => false,
        };
        if !matches {
            return Err(HostError::Call(CallError::InvalidToken));
        }
        self.exception_detection.as_mut().unwrap()[0] = DetectionSlot32::Idle;
        Ok(())
    }

    pub fn is_exception_shutdown(&self) -> bool {
        matches!(
            self.exception_detection.as_ref().map(|slot| &slot[0]),
            Some(DetectionSlot32::Shutdown)
        )
    }

    pub(super) fn latch_exception_shutdown(&mut self) {
        // a consumed record always leaves its already allocated slot; shutdown cannot allocate or fail.
        self.exception_detection
            .as_mut()
            .expect("consumed detection slot")[0] = DetectionSlot32::Shutdown;
    }

    pub(in crate::process) fn has_ready_exception_detection(&self) -> bool {
        matches!(
            self.exception_detection.as_ref().map(|slot| &slot[0]),
            Some(DetectionSlot32::Ready(_))
        )
    }

    fn check_detection_owner(
        &self,
        owner: PendingOwner,
        callback_token: Option<u32>,
    ) -> Result<(), HostError> {
        self.memory()?;
        if self.pending_call.is_some() {
            return Err(HostError::Call(CallError::Busy));
        }
        if self.callback.as_ref().map(|callback| callback.token()) != callback_token {
            return Err(HostError::Call(CallError::InvalidToken));
        }
        match owner {
            PendingOwner::Replacement(generation) => {
                if generation == 0
                    || self.generation() != generation
                    || self
                        .callback
                        .as_ref()
                        .is_some_and(|callback| callback.is_resident())
                {
                    return Err(HostError::InvalidArtifact);
                }
            }
            PendingOwner::Resident(id) => {
                if self.callback.is_some() {
                    return Err(HostError::Call(CallError::Busy));
                }
                self.resident
                    .as_ref()
                    .ok_or(HostError::InvalidArtifact)?
                    .get_retained_raw(self.memory()?, id)
                    .map_err(HostError::Resident)?;
            }
            PendingOwner::ResidentCallback {
                unit_id,
                callback_token,
            } => {
                let callback = self
                    .callback
                    .as_ref()
                    .ok_or(HostError::Call(CallError::InvalidToken))?;
                let record = callback
                    .authorized_resident_record()
                    .ok_or(HostError::Call(CallError::Busy))?;
                if record.token != callback_token
                    || callback.authorized_resident_active_id() != Some(unit_id)
                {
                    return Err(HostError::Call(CallError::InvalidToken));
                }
                let registry = self.resident.as_ref().ok_or(HostError::InvalidArtifact)?;
                for id in [unit_id, record.callback_unit_id, record.outer_unit_id] {
                    registry
                        .get_retained_raw(self.memory()?, id)
                        .map_err(HostError::Resident)?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod integration;
