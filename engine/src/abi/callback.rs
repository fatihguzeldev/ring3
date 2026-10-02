#![forbid(unsafe_code)]

use super::{AbiError, header};
use crate::windows::MAX_STACK_WORDS;

pub const CALLBACK_RECORD_SIZE: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallbackRecord32 {
    pub token: u32,
    pub outer_token: u32,
    pub phase: u32,
    pub outcome: u32,
    pub entry_pc: u32,
    pub entry_esp: u32,
    pub return_pc: u32,
    pub return_id: u32,
    pub stack_words: u32,
    pub result: u32,
    pub generation: u32,
}

pub fn encode_callback(record: &CallbackRecord32, output: &mut [u8]) -> Result<(), AbiError> {
    if output.len() != CALLBACK_RECORD_SIZE {
        return Err(AbiError::Length);
    }
    validate(record)?;
    header::write(output, *b"R3CB");
    for (index, value) in [
        record.token,
        record.outer_token,
        record.phase,
        record.outcome,
        record.entry_pc,
        record.entry_esp,
        record.return_pc,
        record.return_id,
        record.stack_words,
        record.result,
        record.generation,
    ]
    .into_iter()
    .enumerate()
    {
        header::write_u32(output, 16 + index * 4, value);
    }
    Ok(())
}

pub fn decode_callback(input: &[u8]) -> Result<CallbackRecord32, AbiError> {
    header::validate(input, *b"R3CB", CALLBACK_RECORD_SIZE)?;
    if header::read_u32(input, 60) != 0 {
        return Err(AbiError::Reserved);
    }
    let record = CallbackRecord32 {
        token: header::read_u32(input, 16),
        outer_token: header::read_u32(input, 20),
        phase: header::read_u32(input, 24),
        outcome: header::read_u32(input, 28),
        entry_pc: header::read_u32(input, 32),
        entry_esp: header::read_u32(input, 36),
        return_pc: header::read_u32(input, 40),
        return_id: header::read_u32(input, 44),
        stack_words: header::read_u32(input, 48),
        result: header::read_u32(input, 52),
        generation: header::read_u32(input, 56),
    };
    validate(&record)?;
    Ok(record)
}

fn validate(record: &CallbackRecord32) -> Result<(), AbiError> {
    if record.token == 0
        || record.outer_token == 0
        || record.token <= record.outer_token
        || record.generation == 0
        || record.return_id == 0
        || record.stack_words > MAX_STACK_WORDS as u32
        || record.entry_pc == record.return_pc
    {
        return Err(AbiError::CallbackFrame);
    }
    match record.phase {
        1 if record.outcome <= 1 && record.result == 0 => Ok(()),
        2 if record.outcome == 0 => Ok(()),
        _ => Err(AbiError::CallbackFrame),
    }
}
