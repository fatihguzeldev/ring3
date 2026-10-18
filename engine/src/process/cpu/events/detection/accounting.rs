use super::*;

impl EngineInstance {
    pub(in crate::process::cpu::events) fn validate_detected_return_epoch(
        &self,
        epoch: u64,
    ) -> Result<PendingOwner, HostError> {
        self.memory()?;
        if epoch == 0 || epoch != self.detection_epoch {
            return Err(HostError::Call(CallError::InvalidToken));
        }
        if let Some(event) = &self.cpu_event {
            if event.epoch != epoch {
                return Err(HostError::Call(CallError::InvalidToken));
            }
            if !event.matches_context(self.arena()) {
                return Err(HostError::Call(CallError::StateChanged));
            }
            self.check_detection_owner(event.owner, event.callback_token)?;
            return Ok(event.owner);
        }
        let (owner, callback_token) = match self.exception_detection.as_ref().map(|slot| &slot[0]) {
            Some(DetectionSlot32::Ready(record)) if record.epoch == epoch => {
                if !record.matches(self.arena()) {
                    return Err(HostError::Call(CallError::StateChanged));
                }
                (record.owner, record.callback_token)
            }
            Some(DetectionSlot32::Returned(record)) if record.epoch == epoch => {
                if !record.matches(self.arena()) {
                    return Err(HostError::Call(CallError::StateChanged));
                }
                (record.owner, record.callback_token)
            }
            Some(DetectionSlot32::Active {
                epoch: active,
                owner,
                callback_token,
                ..
            }) if *active == epoch => {
                decode_state(&self.arena()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE])
                    .map_err(|_| HostError::Call(CallError::InvalidStop))?;
                decode_exit(&self.arena()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE])
                    .map_err(|_| HostError::Call(CallError::InvalidStop))?;
                ExtendedCpuState::capture_cached(
                    self.arena(),
                    self.validated_extended_state.as_ref(),
                )?;
                let work = &self.arena()[WORK_OFFSET..WORK_OFFSET + WORK_SIZE];
                header::validate(work, *b"R3WK", WORK_SIZE)
                    .map_err(|_| HostError::Call(CallError::InvalidStop))?;
                if header::read_u32(work, 20) != 0 {
                    return Err(HostError::Call(CallError::InvalidStop));
                }
                (*owner, *callback_token)
            }
            _ => return Err(HostError::Call(CallError::InvalidToken)),
        };
        self.check_detection_owner(owner, callback_token)?;
        Ok(owner)
    }
}
