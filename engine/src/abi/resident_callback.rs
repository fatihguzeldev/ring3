#![forbid(unsafe_code)]

use super::{AbiError, header};
use crate::windows::MAX_STACK_WORDS;

pub const RESIDENT_CALLBACK_RECORD_SIZE: usize = 72;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResidentCallbackRecord32 {
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
    pub outer_unit_id: u64,
    pub callback_unit_id: u64,
}

pub fn encode_resident_callback(
    record: &ResidentCallbackRecord32,
    output: &mut [u8],
) -> Result<(), AbiError> {
    if output.len() != RESIDENT_CALLBACK_RECORD_SIZE {
        return Err(AbiError::Length);
    }
    validate(record)?;
    header::write(output, *b"R3RC");
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
        record.outer_unit_id as u32,
        (record.outer_unit_id >> 32) as u32,
        record.callback_unit_id as u32,
        (record.callback_unit_id >> 32) as u32,
    ]
    .into_iter()
    .enumerate()
    {
        header::write_u32(output, 16 + index * 4, value);
    }
    Ok(())
}

pub fn decode_resident_callback(input: &[u8]) -> Result<ResidentCallbackRecord32, AbiError> {
    header::validate(input, *b"R3RC", RESIDENT_CALLBACK_RECORD_SIZE)?;
    let record = ResidentCallbackRecord32 {
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
        outer_unit_id: u64::from(header::read_u32(input, 56))
            | (u64::from(header::read_u32(input, 60)) << 32),
        callback_unit_id: u64::from(header::read_u32(input, 64))
            | (u64::from(header::read_u32(input, 68)) << 32),
    };
    validate(&record)?;
    Ok(record)
}

fn validate(record: &ResidentCallbackRecord32) -> Result<(), AbiError> {
    if record.token == 0
        || record.outer_token == 0
        || record.token <= record.outer_token
        || record.phase != 1
        || record.outcome > 3
        || record.result != 0
        || record.outer_unit_id == 0
        || record.callback_unit_id == 0
        || record.return_id == 0
        || record.stack_words > MAX_STACK_WORDS as u32
        || record.entry_pc == record.return_pc
    {
        return Err(AbiError::CallbackFrame);
    }
    Ok(())
}
