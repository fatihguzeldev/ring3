use super::*;

pub(in crate::process) struct ExecutionDetection32 {
    pub(in crate::process) owner: PendingOwner,
    pub(in crate::process) child_retired: u32,
    pub(in crate::process) kind: u32,
}

fn context_matches(
    arena: &[u8],
    state: &[u8; STATE_SIZE],
    extended: &ExtendedCpuState,
    work: &[u8; WORK_SIZE],
) -> bool {
    arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE] == *state
        && extended.matches(arena)
        && arena[WORK_OFFSET..WORK_OFFSET + WORK_SIZE] == *work
}

fn folded_exit_matches(arena: &[u8], exit: &[u8; EXIT_SIZE]) -> bool {
    arena[EXIT_OFFSET..EXIT_OFFSET + 20] == exit[..20]
        && arena[EXIT_OFFSET + 24..EXIT_OFFSET + EXIT_SIZE] == exit[24..]
}

impl EngineInstance {
    pub(in crate::process) fn prepare_execution_terminal(
        &self,
        epoch: u64,
        total_retired: u32,
        dispatch: bool,
        sealed_retired: u32,
        sealed_work: u32,
        budget: u32,
    ) -> Result<ExecutionDetection32, HostError> {
        if let Some(event) = &self.cpu_event {
            if event.epoch != epoch {
                return Err(HostError::Call(CallError::InvalidToken));
            }
            if !event.matches_folded_context(self.arena()) {
                return Err(HostError::Call(CallError::StateChanged));
            }
            self.check_detection_owner(event.owner, event.callback_token)?;
            if total_retired != sealed_retired {
                return Err(HostError::Call(CallError::InvalidStop));
            }
            return Ok(ExecutionDetection32 {
                owner: event.owner,
                child_retired: event.exit.retired,
                kind: 3,
            });
        }
        let slot = self.exception_detection.as_ref().map(|slot| &slot[0]);
        let (owner, callback_token, child_retired, kind) = match slot {
            Some(DetectionSlot32::Continued(record)) if record.epoch == epoch => {
                // internal authority must be explicitly upgraded before return.
                return Err(HostError::Call(CallError::StateChanged));
            }
            Some(DetectionSlot32::Returned(record)) if record.epoch == epoch => {
                if !context_matches(self.arena(), &record.state, &record.extended, &record.work) {
                    return Err(HostError::Call(CallError::StateChanged));
                }
                if !folded_exit_matches(self.arena(), &record.exit) {
                    let exit = decode_exit(&self.arena()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE])
                        .map_err(|_| HostError::Call(CallError::InvalidStop))?;
                    let child = decode_exit(&record.exit)
                        .map_err(|_| HostError::Call(CallError::InvalidStop))?;
                    let permitted = dispatch
                        && child.reason == ExitReason::NeedCode
                        && match exit.reason {
                            ExitReason::NeedCode => true,
                            ExitReason::Budget => sealed_work == budget,
                            ExitReason::Cancelled => {
                                header::read_u32(self.arena(), crate::abi::arena::CANCEL_OFFSET)
                                    != 0
                            }
                            _ => false,
                        };
                    let mut canonical = [0; EXIT_SIZE];
                    crate::abi::x86::encode_exit_v3(&exit, &mut canonical)
                        .map_err(|_| HostError::Call(CallError::InvalidStop))?;
                    if !permitted || self.arena()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE] != canonical
                    {
                        return Err(HostError::Call(CallError::StateChanged));
                    }
                }
                if total_retired != sealed_retired {
                    return Err(HostError::Call(CallError::InvalidStop));
                }
                (
                    record.owner,
                    record.callback_token,
                    header::read_u32(&record.exit, 20),
                    2,
                )
            }
            Some(DetectionSlot32::Ready(record)) if record.epoch == epoch => {
                if !context_matches(self.arena(), &record.state, &record.extended, &record.work)
                    || !folded_exit_matches(self.arena(), &record.exit)
                {
                    return Err(HostError::Call(CallError::StateChanged));
                }
                if total_retired != sealed_retired {
                    return Err(HostError::Call(CallError::InvalidStop));
                }
                (record.owner, record.callback_token, record.retired, 3)
            }
            Some(DetectionSlot32::Active {
                epoch: active,
                owner,
                callback_token,
                ..
            }) if *active == epoch => {
                self.validate_detected_return_epoch(epoch)?;
                let child = total_retired
                    .checked_sub(sealed_retired)
                    .ok_or(HostError::Call(CallError::InvalidStop))?;
                let work = header::read_u32(self.arena(), crate::abi::arena::WORK_CONSUMED_OFFSET)
                    .max(child);
                if sealed_work
                    .checked_add(work)
                    .is_none_or(|work| work > budget)
                {
                    return Err(HostError::Call(CallError::InvalidStop));
                }
                (*owner, *callback_token, child, 1)
            }
            _ => return Err(HostError::Call(CallError::InvalidToken)),
        };
        self.check_detection_owner(owner, callback_token)?;
        Ok(ExecutionDetection32 {
            owner,
            child_retired,
            kind,
        })
    }

    /// called only after terminal validation; preserve the final child's return authority.
    pub(in crate::process) fn restore_execution_terminal(&mut self, epoch: u64) {
        if let Some(event) = &self.cpu_event {
            debug_assert_eq!(event.epoch, epoch);
            let retired = event.exit.retired;
            header::write_u32(self.arena.as_mut().get_mut(), EXIT_OFFSET + 20, retired);
            return;
        }
        let exit = match self.exception_detection.as_ref().map(|slot| &slot[0]) {
            Some(DetectionSlot32::Continued(_)) => None,
            Some(DetectionSlot32::Returned(record)) if record.epoch == epoch => Some(record.exit),
            Some(DetectionSlot32::Ready(record)) if record.epoch == epoch => Some(record.exit),
            _ => None,
        };
        if let Some(exit) = exit {
            self.arena.as_mut().get_mut()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE]
                .copy_from_slice(&exit);
        }
    }
}
