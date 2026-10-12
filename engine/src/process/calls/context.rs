use crate::process::{CallError, HostError};
use crate::{
    abi::{
        arena::{SEGMENTS_OFFSET, SIMD_OFFSET, STATE_OFFSET, X87_OFFSET},
        x86::{
            SEGMENTS_SIZE, SIMD_SIZE, STATE_SIZE, StateMetadata, X87_SIZE, decode_segment_context,
            decode_simd, decode_state_metadata, decode_x87, encode_segments, encode_simd,
            encode_x87,
        },
    },
    cpu::x86::{SegmentState, X87State, simd::SimdState},
};

/// architectural state outside State32, captured from validated abi records.
/// work accounting and helper/transfer records are runtime outputs, not cpu context.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::process) struct ExtendedCpuState {
    pub(in crate::process) x87: [u8; X87_SIZE],
    simd: [u8; SIMD_SIZE],
    segments: [u8; SEGMENTS_SIZE],
    metadata: StateMetadata,
}

impl ExtendedCpuState {
    pub(in crate::process) fn capture(arena: &[u8]) -> Result<Self, HostError> {
        let state = Self {
            metadata: decode_state_metadata(&arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE])
                .map_err(|_| HostError::Call(CallError::InvalidStop))?,
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
        decode_segment_context(&state.segments)
            .map_err(|_| HostError::Call(CallError::InvalidStop))?;
        Ok(state)
    }

    pub(in crate::process) fn initial() -> Result<Self, HostError> {
        let mut state = Self {
            metadata: StateMetadata::default(),
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
            && decode_state_metadata(&arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE])
                == Ok(self.metadata)
    }

    pub(in crate::process) fn restore(&self, arena: &mut [u8]) {
        arena[X87_OFFSET..X87_OFFSET + X87_SIZE].copy_from_slice(&self.x87);
        arena[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE].copy_from_slice(&self.simd);
        arena[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE].copy_from_slice(&self.segments);
        self.restore_state_metadata(
            (&mut arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE])
                .try_into()
                .unwrap(),
        );
    }

    pub(in crate::process) fn restore_state_metadata(&self, state: &mut [u8; STATE_SIZE]) {
        self.metadata.write(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        abi::{
            arena::EXIT_OFFSET,
            callback::CallbackRecord32,
            x86::{EXIT_SIZE, decode_state},
        },
        cpu::x86::State32,
        process::{
            EngineInstance,
            calls::{
                call::{PendingCall, PendingOwner},
                callback::{SuspendedCallback, SuspendedRecord},
            },
        },
        windows::{CallFrame32, CallingConvention32},
    };

    #[test]
    fn ss_shadow_survives_call_completion_and_suspended_callback_restore() {
        let mut engine = EngineInstance::new(1, 91).unwrap();
        engine.map(0x8000, 1, 3).unwrap();
        engine.write32(0x8010, 0x1000).unwrap();
        let mut state = State32::default();
        state.registers[4] = 0x8010;
        crate::abi::x86::encode_state(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE])
            .unwrap();
        StateMetadata {
            ss_debug_shadow: true,
        }
        .write(
            (&mut engine.arena_mut().unwrap()[..STATE_SIZE])
                .try_into()
                .unwrap(),
        );
        let extended = ExtendedCpuState::capture(engine.arena()).unwrap();
        let frame = CallFrame32::capture(
            engine.memory().unwrap(),
            state,
            CallingConvention32::Cdecl,
            0,
        )
        .unwrap();
        let outer = PendingCall {
            token: 1,
            owner: PendingOwner::Replacement(1),
            frame,
            state: engine.arena()[..STATE_SIZE].try_into().unwrap(),
            exit: engine.arena()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE]
                .try_into()
                .unwrap(),
            extended,
        };
        let expected = outer.state;
        engine.callback = Some(SuspendedCallback {
            record: SuspendedRecord::Replacement(CallbackRecord32 {
                token: 2,
                outer_token: 1,
                phase: 1,
                outcome: 0,
                entry_pc: 0x1100,
                entry_esp: 0x8000,
                return_pc: 0x1200,
                return_id: 18,
                stack_words: 0,
                result: 0,
                generation: 1,
            }),
            outer,
        });
        crate::abi::x86::encode_state(
            &State32::default(),
            &mut engine.arena_mut().unwrap()[..STATE_SIZE],
        )
        .unwrap();
        assert!(!extended.matches(engine.arena()));
        engine.abort_callback(91, 2).unwrap();
        assert_eq!(&engine.arena()[..STATE_SIZE], expected);
        assert!(extended.matches(engine.arena()));
        let completed = frame.complete(42);
        engine.publish_completed_call(completed).unwrap();
        assert_eq!(
            decode_state(&engine.arena()[..STATE_SIZE]).unwrap(),
            completed
        );
        assert_eq!(
            decode_state_metadata(&engine.arena()[..STATE_SIZE]).unwrap(),
            StateMetadata {
                ss_debug_shadow: true
            }
        );
        assert!(engine.pending_call.is_none());
        engine.arena_mut().unwrap()[12] = 2;
        assert_eq!(
            ExtendedCpuState::capture(engine.arena()),
            Err(HostError::Call(CallError::InvalidStop))
        );
    }

    #[test]
    fn cached_segment_record_is_captured_and_restored_without_loss() {
        let mut engine = EngineInstance::new(1, 93).unwrap();
        let at = SEGMENTS_OFFSET;
        let bytes = &mut engine.arena_mut().unwrap()[at..at + SEGMENTS_SIZE];
        bytes[4..6].copy_from_slice(&2_u16.to_le_bytes());
        for (offset, cache) in [
            (28, 0xf33f_ffff_u32),
            (32, 0xf3bf_ffff),
            (36, 0xf33f_ffff),
            (40, 0xf33f_ffff),
            (44, 0xf310_1234),
            (56, 0),
        ] {
            bytes[offset..offset + 4].copy_from_slice(&cache.to_le_bytes());
        }
        let expected: [u8; SEGMENTS_SIZE] = bytes.try_into().unwrap();
        let captured = ExtendedCpuState::capture(engine.arena()).unwrap();
        encode_segments(
            &SegmentState::default(),
            &mut engine.arena_mut().unwrap()[at..at + SEGMENTS_SIZE],
        )
        .unwrap();
        assert!(!captured.matches(engine.arena()));
        captured.restore(engine.arena_mut().unwrap());
        assert_eq!(&engine.arena()[at..at + SEGMENTS_SIZE], expected);
        assert!(captured.matches(engine.arena()));
        engine.arena_mut().unwrap()[at + 63] = 1;
        let malformed = engine.arena().to_vec();
        assert_eq!(
            ExtendedCpuState::capture(engine.arena()),
            Err(HostError::Call(CallError::InvalidStop))
        );
        assert_eq!(engine.arena(), malformed);
    }
}
