#![forbid(unsafe_code)]

use super::{
    AbiError,
    memory_helper::{HELPER_SIZE, encode_helper_result},
    x86::{
        EXIT_SIZE, SEGMENTS_SIZE, SIMD_SIZE, STATE_SIZE, X87_SIZE, encode_exit, encode_segments,
        encode_simd, encode_state, encode_x87,
    },
};
use crate::cpu::{
    ExecutionExit, ExitReason,
    x86::{SegmentState, State32, X87State, simd::SimdState},
};

pub const STATE_OFFSET: usize = 0;
pub const EXIT_OFFSET: usize = 56;
pub const CANCEL_OFFSET: usize = 96;
pub const HELPER_OFFSET: usize = 100;
pub const TRANSFER_OFFSET: usize = 140;
pub const TRANSFER_SIZE: usize = 4096;
pub const X87_OFFSET: usize = TRANSFER_OFFSET + TRANSFER_SIZE;
pub const SIMD_OFFSET: usize = X87_OFFSET + X87_SIZE;
pub const SEGMENTS_OFFSET: usize = SIMD_OFFSET + SIMD_SIZE;
pub const WORK_OFFSET: usize = SEGMENTS_OFFSET + SEGMENTS_SIZE;
pub const WORK_SIZE: usize = 24;
pub const WORK_CONSUMED_OFFSET: usize = WORK_OFFSET + 16;
pub const ARENA_SIZE: usize = WORK_OFFSET + WORK_SIZE;

pub(crate) fn initialize(output: &mut [u8]) -> Result<(), AbiError> {
    if output.len() != ARENA_SIZE {
        return Err(AbiError::Length);
    }
    output.fill(0);
    encode_state(
        &State32::default(),
        &mut output[STATE_OFFSET..STATE_OFFSET + STATE_SIZE],
    )?;
    encode_exit(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Budget,
        },
        &mut output[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )?;
    encode_helper_result(
        Ok(0),
        &mut output[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE],
    )?;
    encode_x87(
        &X87State::default(),
        &mut output[X87_OFFSET..X87_OFFSET + X87_SIZE],
    )?;
    encode_simd(
        &SimdState::default(),
        &mut output[SIMD_OFFSET..SIMD_OFFSET + SIMD_SIZE],
    )?;
    encode_segments(
        &SegmentState::default(),
        &mut output[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE],
    )?;
    super::header::write(&mut output[WORK_OFFSET..WORK_OFFSET + WORK_SIZE], *b"R3WK");
    Ok(())
}
