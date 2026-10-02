#![forbid(unsafe_code)]

use super::{AbiError, header};
use crate::windows::{CallingConvention32, MAX_STACK_WORDS};

pub const CALL_FRAME_SIZE: usize = 112;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallRecord32 {
    pub token: u32,
    pub id: u32,
    pub convention: u32,
    pub stack_words: u32,
    pub gate_pc: u32,
    pub entry_esp: u32,
    pub return_pc: u32,
    pub this_pointer: u32,
    pub arguments: [u32; MAX_STACK_WORDS],
}

pub fn encode_call_frame(record: &CallRecord32, output: &mut [u8]) -> Result<(), AbiError> {
    if output.len() != CALL_FRAME_SIZE {
        return Err(AbiError::Length);
    }
    validate(record)?;
    header::write(output, *b"R3CF");
    for (index, value) in [
        record.token,
        record.id,
        record.convention,
        record.stack_words,
        record.gate_pc,
        record.entry_esp,
        record.return_pc,
        record.this_pointer,
    ]
    .into_iter()
    .enumerate()
    {
        header::write_u32(output, 16 + index * 4, value);
    }
    for (index, value) in record.arguments.iter().enumerate() {
        header::write_u32(output, 48 + index * 4, *value);
    }
    Ok(())
}

pub fn decode_call_frame(input: &[u8]) -> Result<CallRecord32, AbiError> {
    header::validate(input, *b"R3CF", CALL_FRAME_SIZE)?;
    let mut arguments = [0; MAX_STACK_WORDS];
    for (index, value) in arguments.iter_mut().enumerate() {
        *value = header::read_u32(input, 48 + index * 4);
    }
    let record = CallRecord32 {
        token: header::read_u32(input, 16),
        id: header::read_u32(input, 20),
        convention: header::read_u32(input, 24),
        stack_words: header::read_u32(input, 28),
        gate_pc: header::read_u32(input, 32),
        entry_esp: header::read_u32(input, 36),
        return_pc: header::read_u32(input, 40),
        this_pointer: header::read_u32(input, 44),
        arguments,
    };
    validate(&record)?;
    Ok(record)
}

fn validate(record: &CallRecord32) -> Result<(), AbiError> {
    let convention =
        CallingConvention32::try_from(record.convention).map_err(|_| AbiError::CallFrame)?;
    if record.token == 0
        || record.id == 0
        || record.stack_words > MAX_STACK_WORDS as u32
        || (convention != CallingConvention32::Thiscall && record.this_pointer != 0)
    {
        return Err(AbiError::CallFrame);
    }
    if record.arguments[record.stack_words as usize..]
        .iter()
        .any(|argument| *argument != 0)
    {
        return Err(AbiError::CallFrame);
    }
    Ok(())
}
