use crate::process::{CallError, HostError};
use crate::{
    abi::{
        arena::{SEGMENTS_OFFSET, SIMD_OFFSET, X87_OFFSET},
        x86::{
            SEGMENTS_SIZE, SIMD_SIZE, X87_SIZE, decode_segments, decode_simd, decode_x87,
            encode_segments, encode_simd, encode_x87,
        },
    },
    cpu::x86::{SegmentState, X87State, simd::SimdState},
};

/// The architectural state outside R3ST, captured as exact validated ABI bytes.
/// Work accounting and helper/transfer records are runtime outputs, not CPU context.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::process) struct ExtendedCpuState {
    pub(in crate::process) x87: [u8; X87_SIZE],
    simd: [u8; SIMD_SIZE],
    segments: [u8; SEGMENTS_SIZE],
}

impl ExtendedCpuState {
    pub(in crate::process) fn capture(arena: &[u8]) -> Result<Self, HostError> {
        let state = Self {
            x87: arena[X87_OFFSET..X87_OFFSET + X87_SIZE].try_into().unwrap(),
            simd: arena[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE]
                .try_into()
                .unwrap(),
            segments: arena[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE]
                .try_into()
                .unwrap(),
        };
        decode_x87(&state.x87).map_err(|_| HostError::Call(CallError::InvalidStop))?;
        decode_simd(&state.simd).map_err(|_| HostError::Call(CallError::InvalidStop))?;
        decode_segments(&state.segments).map_err(|_| HostError::Call(CallError::InvalidStop))?;
        Ok(state)
    }

    pub(in crate::process) fn initial() -> Result<Self, HostError> {
        let mut state = Self {
            x87: [0; X87_SIZE],
            simd: [0; SIMD_SIZE],
            segments: [0; SEGMENTS_SIZE],
        };
        encode_x87(&X87State::default(), &mut state.x87).map_err(|_| HostError::Infrastructure)?;
        encode_simd(&SimdState::default(), &mut state.simd)
            .map_err(|_| HostError::Infrastructure)?;
        encode_segments(&SegmentState::default(), &mut state.segments)
            .map_err(|_| HostError::Infrastructure)?;
        Ok(state)
    }

    pub(in crate::process) fn matches(&self, arena: &[u8]) -> bool {
        arena[X87_OFFSET..X87_OFFSET + X87_SIZE] == self.x87
            && arena[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE] == self.simd
            && arena[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE] == self.segments
    }

    pub(in crate::process) fn restore(&self, arena: &mut [u8]) {
        arena[X87_OFFSET..X87_OFFSET + X87_SIZE].copy_from_slice(&self.x87);
        arena[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE].copy_from_slice(&self.simd);
        arena[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE].copy_from_slice(&self.segments);
    }
}
