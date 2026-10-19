use crate::{
    abi::{arena::*, x86::*},
    cpu::x86::SegmentRegister,
    process::{EngineInstance, HostError},
    windows::exceptions::{
        ExceptionError32,
        context::{ContextState32, Win32Context32},
    },
};
fn invalid() -> HostError {
    HostError::Exception(ExceptionError32::InvalidContext)
}
impl EngineInstance {
    pub(in crate::process) fn windows_context_state(&self) -> Result<ContextState32, HostError> {
        let arena = self.arena();
        Ok(ContextState32 {
            state: decode_state(&arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE])
                .map_err(|_| invalid())?,
            segments: self.segment_context()?,
            x87: decode_x87(&arena[X87_OFFSET..X87_OFFSET + X87_SIZE]).map_err(|_| invalid())?,
            simd: decode_simd(&arena[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE])
                .map_err(|_| invalid())?,
            debug: [0; 6],
        })
    }
    pub(in crate::process) fn apply_windows_context(
        &self,
        context: &Win32Context32,
        base: &ContextState32,
    ) -> Result<ContextState32, HostError> {
        // The fixed CPL3 execution profile has no hardware debug-register bank.
        if context.requested_debug_registers(base) != [0; 6] {
            return Err(invalid());
        }
        let mut segments = base.segments;
        for (index, selector) in context.requested_selectors(base).into_iter().enumerate() {
            if selector == segments.state.selectors[index] {
                continue;
            }
            let register = [
                SegmentRegister::Es,
                SegmentRegister::Cs,
                SegmentRegister::Ss,
                SegmentRegister::Ds,
                SegmentRegister::Fs,
                SegmentRegister::Gs,
            ][index];
            if register == SegmentRegister::Cs {
                self.segment_descriptors
                    .resolve_far_jump(
                        selector,
                        context.requested_eip(base),
                        crate::cpu::x86::FarWidth::Dword,
                        3,
                    )
                    .map_err(|_| invalid())?
                    .apply(&mut segments);
            } else {
                self.segment_descriptors
                    .resolve_load(register, selector, 3, &segments.state)
                    .map_err(|_| invalid())?
                    .apply_context(&mut segments, register);
            }
        }
        context.apply(base, segments).map_err(|_| invalid())
    }
    pub(in crate::process) fn publish_windows_context(
        &mut self,
        state: &ContextState32,
    ) -> Result<(), HostError> {
        if state.debug != [0; 6] {
            return Err(invalid());
        }
        let mut integer = [0; STATE_SIZE];
        let mut x87 = [0; X87_SIZE];
        let mut simd = [0; SIMD_SIZE];
        let mut segments = [0; SEGMENTS_SIZE];
        let mut bases = [0; SEGMENT_BASES_RECORD_SIZE];
        encode_state_current(&state.state, &mut integer).map_err(|_| invalid())?;
        encode_x87(&state.x87, &mut x87).map_err(|_| invalid())?;
        encode_simd(&state.simd, &mut simd).map_err(|_| invalid())?;
        encode_current_segment_context(&state.segments, &mut segments, &mut bases)
            .map_err(|_| invalid())?;
        self.revoke_owned_execution();
        let arena = self.arena.as_mut().get_mut();
        arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE].copy_from_slice(&integer);
        arena[X87_OFFSET..X87_OFFSET + X87_SIZE].copy_from_slice(&x87);
        arena[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE].copy_from_slice(&simd);
        arena[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE].copy_from_slice(&segments);
        arena[SEGMENT_BASES_RECORD_OFFSET..SEGMENT_BASES_RECORD_OFFSET + SEGMENT_BASES_RECORD_SIZE]
            .copy_from_slice(&bases);
        Ok(())
    }
}
