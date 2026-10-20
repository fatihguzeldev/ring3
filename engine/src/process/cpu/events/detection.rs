use crate::{
    abi::{
        arena::{EXIT_OFFSET, STATE_OFFSET, WORK_OFFSET, WORK_SIZE},
        header,
        x86::{EXIT_SIZE, STATE_SIZE, decode_exit, decode_state, decode_state_metadata},
    },
    cpu::{
        ExecutionExit, ExitReason,
        x86::{
            EventOrigin32, ExceptionCause32, FrameInput32, Register32, SegmentRegister,
            VerifiedPageFault32,
        },
    },
    memory::MemoryFault,
    process::{
        CallError, EngineInstance, HostError, calls::call::PendingOwner,
        cpu::context::ExtendedCpuState,
    },
};

mod accounting;
mod paging;
mod restart;
#[cfg(any(target_arch = "wasm32", test))]
mod returning;
pub(in crate::process) mod site;
pub(in crate::process) use restart::InstructionTrapRestart32;

/// detector-supplied cause and precise application access-fault provenance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::process) enum DetectedEvent32 {
    Exception(EventOrigin32),
    MemoryFailure { fault: MemoryFault, length: u32 },
}

// the owner boxes this slot once; keeping ready inline avoids an allocation at every fault.
#[allow(clippy::large_enum_variant)]
pub(in crate::process) enum DetectionSlot32 {
    Idle,
    Active {
        epoch: u64,
        owner: PendingOwner,
        callback_token: Option<u32>,
        page_fault: Option<VerifiedPageFault32>,
    },
    Ready(ConsumedDetection32),
    Returned(ReturnedDetection32),
    Continued(ContinuedDetection32),
}

pub(in crate::process) struct ReturnedDetection32 {
    epoch: u64,
    owner: PendingOwner,
    callback_token: Option<u32>,
    state: [u8; STATE_SIZE],
    exit: [u8; EXIT_SIZE],
    extended: ExtendedCpuState,
    work: [u8; WORK_SIZE],
}

impl ReturnedDetection32 {
    fn matches(&self, arena: &[u8]) -> bool {
        arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE] == self.state
            && arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE] == self.exit
            && self.extended.matches(arena)
            && arena[WORK_OFFSET..WORK_OFFSET + WORK_SIZE] == self.work
    }
}

// a private internal continuation contains no extended-bank payload. this is
// deliberately a separate detector variant, not a large returned record whose
// optional extended field would still be copied during enum publication.
#[derive(Clone, Copy)]
pub(in crate::process) struct ContinuedDetection32 {
    epoch: u64,
    owner: PendingOwner,
    callback_token: Option<u32>,
    state: [u8; STATE_SIZE],
    exit: [u8; EXIT_SIZE],
    work: [u8; WORK_SIZE],
    compiled_token: u64,
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
    privileged_instruction: bool,
    guard_page: Option<crate::memory::GuardPage32>,
}

impl ConsumedDetection32 {
    pub(in crate::process) fn into_event(self) -> crate::process::CpuEvent32 {
        use crate::abi::x86::{
            decode_current_segment_context, decode_simd, decode_x87, decode_ymm,
        };
        crate::process::CpuEvent32 {
            owner: self.owner,
            callback_token: self.callback_token,
            epoch: self.epoch,
            origin: match self.event {
                DetectedEvent32::Exception(origin) => Some(origin),
                DetectedEvent32::MemoryFailure { .. } => None,
            },
            instruction: self
                .restart
                .map(|site| crate::process::CpuInstructionSite32 {
                    eip: site.eip,
                    retired: site.retired,
                    flags: site.flags,
                    ss_debug_shadow: site.ss_debug_shadow,
                    work_consumed: site.work_consumed,
                }),
            state: crate::cpu::x86::State32 {
                eip: self.input.eip,
                ..decode_state(&self.state).unwrap()
            },
            exit: ExecutionExit {
                retired: self.retired,
                ..decode_exit(&self.exit).unwrap()
            },
            ss_debug_shadow: self.extended.metadata.ss_debug_shadow,
            x87: decode_x87(&self.extended.x87).unwrap(),
            simd: decode_simd(&self.extended.simd).unwrap(),
            segments: decode_current_segment_context(
                &self.extended.segments,
                self.extended.segment_bases.as_ref().map(|v| v.as_slice()),
            )
            .unwrap(),
            ymm: decode_ymm(&self.extended.upper).unwrap(),
            work_consumed: header::read_u32(&self.work, 16),
            privileged_instruction: self.privileged_instruction,
            guard_page: self.guard_page,
        }
    }
    #[cfg(test)]
    pub(in crate::process) const fn epoch(&self) -> u64 {
        self.epoch
    }
    #[cfg(test)]
    pub(in crate::process) const fn owner(&self) -> PendingOwner {
        self.owner
    }
    #[cfg(test)]
    pub(in crate::process) const fn event(&self) -> DetectedEvent32 {
        self.event
    }
    #[cfg(test)]
    pub(in crate::process) const fn input(&self) -> FrameInput32 {
        self.input
    }
    #[cfg(test)]
    pub(in crate::process) const fn instruction_restart(&self) -> Option<InstructionTrapRestart32> {
        self.restart
    }
    #[cfg(test)]
    pub(in crate::process) const fn retired(&self) -> u32 {
        self.retired
    }
    #[cfg(test)]
    pub(in crate::process) fn state(&self) -> &[u8; STATE_SIZE] {
        &self.state
    }
    #[cfg(test)]
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
                Self::MemoryFailure { fault, length },
                ExitReason::MemoryFault {
                    fault: actual,
                    length: actual_length,
                },
            ) => fault == actual && length == actual_length,
            (
                Self::Exception(EventOrigin32::SystemEntry(kind)),
                ExitReason::SystemEntry { kind: actual },
            ) => kind == actual,
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
                (
                    ExceptionCause32::PageFault(fault),
                    ExitReason::PageFault {
                        error_code,
                        linear,
                        access,
                        ..
                    },
                ) => {
                    fault.error_code() == error_code
                        && fault.linear() == linear
                        && paging::access(fault.access()) == access
                }
                _ => false,
            },
            _ => false,
        }
    }
}

impl EngineInstance {
    /// the bound entry guard has already validated material identity and lifetime.
    /// an unfinished lease is the explicit non-event-return cleanup on the next valid entry.
    pub(in crate::process) fn begin_exception_detection(
        &mut self,
        owner: PendingOwner,
    ) -> Result<u64, HostError> {
        self.memory()?;
        if self.has_retained_detection() {
            return Err(HostError::Call(CallError::Busy));
        }
        let callback_token = self.callback.as_ref().map(|callback| callback.token());
        self.check_detection_owner(owner, callback_token)?;
        #[cfg(any(target_arch = "wasm32", test))]
        let installed = self.authorize_execution_child(owner)?;
        let epoch = self
            .detection_epoch
            .checked_add(1)
            .filter(|value| *value <= i64::MAX as u64)
            .ok_or(HostError::GenerationExhausted)?;
        #[cfg(any(target_arch = "wasm32", test))]
        let compiled = self.prepare_compiled_child(owner)?;
        let active = DetectionSlot32::Active {
            epoch,
            owner,
            callback_token,
            page_fault: None,
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
        self.cpu_event = None;
        #[cfg(any(target_arch = "wasm32", test))]
        self.entered_execution_child(epoch, installed);
        #[cfg(any(target_arch = "wasm32", test))]
        self.entered_compiled_child(compiled);
        self.refresh_cpu_read32_view();
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
                ..
            }) if *active == epoch => (*owner, *callback_token),
            _ => return Err(HostError::Call(CallError::InvalidToken)),
        };
        self.check_detection_owner(owner, callback_token)?;
        self.check_page_fault_detection32(epoch, event)?;
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
        let context = self.segment_context()?;
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
        let privileged_instruction = if matches!(
            event,
            DetectedEvent32::Exception(EventOrigin32::Exception(
                ExceptionCause32::GeneralProtection(_)
            ))
        ) {
            let artifact = match owner {
                PendingOwner::Core(_) => return Err(HostError::InvalidArtifact),
                PendingOwner::Replacement(_) => {
                    self.artifact.as_ref().ok_or(HostError::InvalidArtifact)?
                }
                PendingOwner::Resident(id) | PendingOwner::ResidentCallback { unit_id: id, .. } => {
                    self.resident
                        .as_ref()
                        .ok_or(HostError::InvalidArtifact)?
                        .get_retained_raw(self.memory()?, id)
                        .map_err(HostError::Resident)?
                }
            };
            artifact.is_privileged_instruction_at(input.eip)
        } else {
            false
        };
        let guard_page = match event {
            DetectedEvent32::MemoryFailure { fault, .. }
                if fault.reason == crate::memory::FaultReason::Permission =>
            {
                self.memory()?.guard_page(fault.address)
            }
            DetectedEvent32::Exception(EventOrigin32::Exception(ExceptionCause32::PageFault(
                fault,
            ))) => self.memory()?.guard_page(fault.linear()),
            _ => None,
        };
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
            privileged_instruction,
            guard_page,
        };
        self.exception_detection.as_mut().unwrap()[0] = DetectionSlot32::Ready(record);
        self.refresh_cpu_read32_view();
        Ok(())
    }

    /// retires a flushed non-event stop without publishing a pending access-fault witness.
    pub(super) fn finish_need_code_detection(&mut self, epoch: u64) -> Result<(), HostError> {
        self.memory()?;
        let (owner, callback_token) = match self.exception_detection.as_ref().map(|slot| &slot[0]) {
            Some(DetectionSlot32::Active {
                epoch: active,
                owner,
                callback_token,
                ..
            }) if *active == epoch => (*owner, *callback_token),
            _ => return Err(HostError::Call(CallError::InvalidToken)),
        };
        self.check_detection_owner(owner, callback_token)?;
        let arena = self.arena();
        decode_state(&arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE])
            .map_err(|_| HostError::Call(CallError::InvalidStop))?;
        let exit = decode_exit(&arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE])
            .map_err(|_| HostError::Call(CallError::InvalidStop))?;
        if exit.reason != ExitReason::NeedCode {
            return Err(HostError::Call(CallError::InvalidStop));
        }
        if let Some(compiled_token) = self.compiled_need_code_token(epoch) {
            // flush changes state metadata in any instruction. decode it freshly;
            // then publish only the small variant and leave the full path here.
            decode_state_metadata(&arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE])
                .map_err(|_| HostError::Call(CallError::InvalidStop))?;
            let work = &arena[WORK_OFFSET..WORK_OFFSET + WORK_SIZE];
            header::validate(work, *b"R3WK", WORK_SIZE)
                .map_err(|_| HostError::Call(CallError::InvalidStop))?;
            if header::read_u32(work, 20) != 0 {
                return Err(HostError::Call(CallError::InvalidStop));
            }
            let continued = ContinuedDetection32 {
                epoch,
                owner,
                callback_token,
                state: arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE]
                    .try_into()
                    .unwrap(),
                exit: arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE]
                    .try_into()
                    .unwrap(),
                work: work.try_into().unwrap(),
                compiled_token,
            };
            self.exception_detection.as_mut().unwrap()[0] = DetectionSlot32::Continued(continued);
            self.committed_compiled_need_code(true);
            self.refresh_cpu_read32_view();
            return Ok(());
        }
        let extended = ExtendedCpuState::capture(arena)?;
        let work = &arena[WORK_OFFSET..WORK_OFFSET + WORK_SIZE];
        header::validate(work, *b"R3WK", WORK_SIZE)
            .map_err(|_| HostError::Call(CallError::InvalidStop))?;
        if header::read_u32(work, 20) != 0 {
            return Err(HostError::Call(CallError::InvalidStop));
        }
        // cr2 and its occurrence sequence describe the real native access observation.
        let returned = ReturnedDetection32 {
            epoch,
            owner,
            callback_token,
            state: arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE]
                .try_into()
                .unwrap(),
            exit: arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE]
                .try_into()
                .unwrap(),
            extended,
            work: work.try_into().unwrap(),
        };
        self.exception_detection.as_mut().unwrap()[0] = DetectionSlot32::Returned(returned);
        self.committed_compiled_need_code(false);
        self.refresh_cpu_read32_view();
        Ok(())
    }

    pub(in crate::process) fn has_compiled_light_continuation(&self) -> bool {
        matches!(
            self.exception_detection.as_ref().map(|slot| &slot[0]),
            Some(DetectionSlot32::Continued(_))
        )
    }

    /// explicit fallible upgrade. never called implicitly by void revocation.
    /// canonical banks remain live and fully published by ordinary x87 helpers.
    pub(in crate::process) fn materialize_compiled_continuation(
        &mut self,
    ) -> Result<(), HostError> {
        let continued = match self.exception_detection.as_ref().map(|slot| &slot[0]) {
            Some(DetectionSlot32::Continued(record)) => *record,
            _ => return Ok(()),
        };
        if !self.compiled_continuation_current(
            continued.compiled_token,
            continued.epoch,
            continued.owner,
        ) {
            return Err(HostError::Call(CallError::StateChanged));
        }
        self.check_detection_owner(continued.owner, continued.callback_token)?;
        let extended = ExtendedCpuState::capture(self.arena())?;
        let returned = ReturnedDetection32 {
            epoch: continued.epoch,
            owner: continued.owner,
            callback_token: continued.callback_token,
            state: continued.state,
            exit: continued.exit,
            work: continued.work,
            extended,
        };
        self.exception_detection.as_mut().unwrap()[0] = DetectionSlot32::Returned(returned);
        let count = &self.compiled_execution_stats.full_materializations;
        count.set(count.get().saturating_add(1));
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
        if !record.matches(self.arena()) {
            return Err(HostError::Call(CallError::StateChanged));
        }
        if let DetectedEvent32::Exception(EventOrigin32::Exception(ExceptionCause32::PageFault(
            fault,
        ))) = record.event
            && self.cr2() != Some(fault.linear())
        {
            return Err(HostError::Call(CallError::StateChanged));
        }
        let DetectionSlot32::Ready(record) = std::mem::replace(
            &mut self.exception_detection.as_mut().unwrap()[0],
            DetectionSlot32::Idle,
        ) else {
            unreachable!("ready record checked before infallible single consumption")
        };
        self.refresh_cpu_read32_view();
        Ok(record)
    }

    /// test-only abandonment; production consumes ready atomically or replaces an active non-event lease.
    #[cfg(test)]
    pub(in crate::process) fn discard_exception_detection(
        &mut self,
        epoch: u64,
    ) -> Result<(), HostError> {
        self.memory()?;
        let matches = match self.exception_detection.as_ref().map(|slot| &slot[0]) {
            Some(DetectionSlot32::Active { epoch: active, .. }) => *active == epoch,
            Some(DetectionSlot32::Ready(record)) => record.epoch == epoch,
            _ => false,
        };
        if !matches {
            return Err(HostError::Call(CallError::InvalidToken));
        }
        self.exception_detection.as_mut().unwrap()[0] = DetectionSlot32::Idle;
        self.refresh_cpu_read32_view();
        Ok(())
    }

    pub(in crate::process) fn clear_certified_page_fault32(&mut self) {
        if let Some(DetectionSlot32::Active { page_fault, .. }) =
            self.exception_detection.as_mut().map(|slot| &mut slot[0])
            && page_fault.take().is_some()
        {
            self.refresh_cpu_read32_view();
        }
    }

    pub(in crate::process) fn observe_certified_page_fault32(
        &mut self,
        fault: VerifiedPageFault32,
    ) {
        if let Some(DetectionSlot32::Active { page_fault, .. }) =
            self.exception_detection.as_mut().map(|slot| &mut slot[0])
        {
            *page_fault = Some(fault);
            self.revoke_cpu_read32_view();
        }
    }

    pub(in crate::process) fn certified_page_fault32(
        &self,
        epoch: u64,
    ) -> Option<VerifiedPageFault32> {
        match self.exception_detection.as_ref().map(|slot| &slot[0]) {
            Some(DetectionSlot32::Active {
                epoch: active,
                page_fault,
                ..
            }) if *active == epoch => *page_fault,
            _ => None,
        }
    }

    pub(in crate::process) fn has_ready_exception_detection(&self) -> bool {
        matches!(
            self.exception_detection.as_ref().map(|slot| &slot[0]),
            Some(DetectionSlot32::Ready(_))
        )
    }

    pub(in crate::process) fn has_retained_detection(&self) -> bool {
        self.has_certified_page_fault32() || self.has_ready_exception_detection()
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
            PendingOwner::Core(id) => {
                self.guard_core_owner(id)?;
                if self
                    .callback
                    .as_ref()
                    .is_some_and(|callback| !callback.is_core())
                {
                    return Err(HostError::Call(CallError::Busy));
                }
            }
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
