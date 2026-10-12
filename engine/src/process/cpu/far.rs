use crate::{
    abi::{
        arena::{HELPER_OFFSET, SEGMENTS_OFFSET},
        cpu_helper::{CpuMemoryError, encode_cpu_result},
        memory_helper::HELPER_SIZE,
        x86::{SEGMENTS_SIZE, encode_segment_context},
    },
    cpu::x86::{CodeTransferError, FarWidth, SegmentRegister, USER_MODE_ENVIRONMENT},
    process::{EngineInstance, HostError},
};

impl EngineInstance {
    pub fn far_jump(&mut self, offset: u32, selector: u32, word: u32) -> Result<(), HostError> {
        self.guard(self.key, self.generation())?;
        self.far_jump_bound(offset, selector, word)
    }

    pub fn far_jump_resident(
        &mut self,
        key: u64,
        id: u64,
        offset: u32,
        selector: u32,
        word: u32,
    ) -> Result<(), HostError> {
        self.guard_resident(key, id)?;
        self.far_jump_bound(offset, selector, word)
    }

    fn far_jump_bound(&mut self, offset: u32, selector: u32, word: u32) -> Result<(), HostError> {
        let width = match word {
            0 => FarWidth::Dword,
            1 => FarWidth::Word,
            _ => return Err(HostError::InvalidRequest),
        };
        let selector = u16::try_from(selector).map_err(|_| HostError::InvalidRequest)?;
        let mut context = self.segment_context()?;
        let result = self.segment_descriptors.resolve_far_jump(
            selector,
            offset,
            width,
            USER_MODE_ENVIRONMENT.current_privilege(),
        );
        let mut packet = [0; HELPER_SIZE];
        match result {
            Ok(transfer) => {
                let cs = SegmentRegister::Cs.index();
                if context.state.bases[cs] != 0
                    || !context.caches[cs].default32()
                    || context.caches[cs].limit() != u32::MAX
                {
                    return Err(HostError::InvalidRequest);
                }
                transfer.apply(&mut context);
                let mut segments = [0; SEGMENTS_SIZE];
                encode_segment_context(&context, &mut segments)
                    .map_err(|_| HostError::Infrastructure)?;
                encode_cpu_result(0, transfer.offset, Ok(()), &mut packet)
                    .map_err(|_| HostError::Infrastructure)?;
                // guard validated the retained artifact before resolution. This synchronous borrow
                // performs no code write or reentrant call; every remaining publication is infallible.
                self.segment_descriptors
                    .commit_accessed(transfer.selector());
                self.arena.as_mut().get_mut()[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE]
                    .copy_from_slice(&segments);
            }
            Err(CodeTransferError::UnsupportedProfile) => return Err(HostError::InvalidRequest),
            Err(error) => {
                let error = match error {
                    CodeTransferError::GeneralProtection(code) => {
                        CpuMemoryError::GeneralProtection(code)
                    }
                    CodeTransferError::NotPresent(code) => CpuMemoryError::SegmentNotPresent(code),
                    CodeTransferError::UnsupportedProfile => unreachable!(),
                };
                encode_cpu_result(0, 0, Err(error), &mut packet)
                    .map_err(|_| HostError::Infrastructure)?;
            }
        }
        self.arena.as_mut().get_mut()[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE]
            .copy_from_slice(&packet);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
