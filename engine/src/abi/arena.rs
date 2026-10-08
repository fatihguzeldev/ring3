#![forbid(unsafe_code)]

use super::{
    AbiError,
    memory_helper::{HELPER_SIZE, encode_helper_result},
    x86::{EXIT_SIZE, STATE_SIZE, X87_SIZE, encode_exit, encode_state, encode_x87},
};
use crate::cpu::{
    ExecutionExit, ExitReason,
    x86::{State32, X87State},
};

pub const STATE_OFFSET: usize = 0;
pub const EXIT_OFFSET: usize = 56;
pub const CANCEL_OFFSET: usize = 96;
pub const HELPER_OFFSET: usize = 100;
pub const TRANSFER_OFFSET: usize = 140;
pub const TRANSFER_SIZE: usize = 4096;
pub const X87_OFFSET: usize = TRANSFER_OFFSET + TRANSFER_SIZE;
pub const ARENA_SIZE: usize = X87_OFFSET + X87_SIZE;

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
    )
}
