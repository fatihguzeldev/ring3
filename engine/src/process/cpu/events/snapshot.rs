use crate::{
    cpu::{
        ExecutionExit,
        x86::{
            EventOrigin32, SegmentContext, State32, X87State,
            simd::{SimdState, ymm::YmmUpperState},
        },
    },
    process::{EngineInstance, HostError},
};

/// original instruction state for a completed instruction-origin trap or os entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpuInstructionSite32 {
    pub eip: u32,
    pub retired: u32,
    pub flags: u32,
    pub ss_debug_shadow: bool,
    pub work_consumed: Option<u32>,
}

/// immutable cpu output for an os exception or service provider; not a resume token.
pub struct CpuEvent32 {
    pub(in crate::process) owner: crate::process::calls::call::PendingOwner,
    pub(in crate::process) callback_token: Option<u32>,
    pub epoch: u64,
    pub origin: Option<EventOrigin32>,
    pub instruction: Option<CpuInstructionSite32>,
    pub state: State32,
    pub exit: ExecutionExit,
    pub ss_debug_shadow: bool,
    pub x87: X87State,
    pub simd: SimdState,
    pub segments: SegmentContext,
    pub ymm: YmmUpperState,
    pub work_consumed: u32,
    pub(in crate::process) privileged_instruction: bool,
    pub(in crate::process) guard_page: Option<crate::memory::GuardPage32>,
}

impl CpuEvent32 {
    /// class of the admitted instruction that produced this sealed event, independent of later code writes.
    pub fn is_privileged_instruction(&self) -> bool {
        self.privileged_instruction
    }

    pub(in crate::process) fn matches_context(&self, arena: &[u8]) -> bool {
        self.matches_execution_context(arena, false)
    }

    #[cfg(any(target_arch = "wasm32", test))]
    pub(in crate::process) fn matches_folded_context(&self, arena: &[u8]) -> bool {
        self.matches_execution_context(arena, true)
    }

    fn matches_execution_context(&self, arena: &[u8], folded_retired: bool) -> bool {
        use crate::abi::{arena::*, x86::*};
        let exit = decode_exit(&arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE]).map(|mut exit| {
            if folded_retired {
                exit.retired = self.exit.retired;
            }
            exit
        });
        decode_state(&arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE]) == Ok(self.state)
            && exit == Ok(self.exit)
            && decode_state_metadata(&arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE])
                .is_ok_and(|metadata| metadata.ss_debug_shadow == self.ss_debug_shadow)
            && decode_x87(&arena[X87_OFFSET..X87_OFFSET + X87_SIZE]) == Ok(self.x87)
            && decode_simd(&arena[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE]) == Ok(self.simd)
            && decode_ymm(&arena[YMM_OFFSET..YMM_OFFSET + YMM_SIZE]) == Ok(self.ymm)
            && decode_current_segment_context(
                &arena[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE],
                Some(
                    &arena[SEGMENT_BASES_RECORD_OFFSET
                        ..SEGMENT_BASES_RECORD_OFFSET + SEGMENT_BASES_RECORD_SIZE],
                ),
            ) == Ok(self.segments)
            && crate::abi::header::validate(
                &arena[WORK_OFFSET..WORK_OFFSET + WORK_SIZE],
                *b"R3WK",
                WORK_SIZE,
            )
            .is_ok()
            && crate::abi::header::read_u32(arena, WORK_OFFSET + 20) == 0
            && crate::abi::header::read_u32(arena, WORK_OFFSET + 16) == self.work_consumed
    }
}

impl EngineInstance {
    /// validates a trusted host's post-return duration claim before another execution entry.
    pub(in crate::process) fn validate_execution_return_epoch(
        &self,
        epoch: u64,
    ) -> Result<crate::process::calls::call::PendingOwner, HostError> {
        self.memory()?;
        #[cfg(any(target_arch = "wasm32", test))]
        {
            self.validate_detected_return_epoch(epoch)
        }
        #[cfg(not(any(target_arch = "wasm32", test)))]
        {
            let event = self
                .cpu_event
                .as_ref()
                .filter(|event| event.epoch == epoch && epoch != 0)
                .ok_or(HostError::Call(crate::process::CallError::InvalidToken))?;
            if !event.matches_context(self.arena()) {
                return Err(HostError::Call(crate::process::CallError::StateChanged));
            }
            if self.callback.as_ref().map(|callback| callback.token()) != event.callback_token {
                return Err(HostError::Call(crate::process::CallError::InvalidToken));
            }
            Ok(event.owner)
        }
    }

    /// takes the last validated stop once, before the next execution entry replaces it.
    pub fn take_cpu_event32(&mut self) -> Result<Option<CpuEvent32>, HostError> {
        self.memory()?;
        Ok(self.cpu_event.take())
    }
}
