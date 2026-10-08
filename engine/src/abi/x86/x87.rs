use crate::abi::AbiError;
use crate::abi::header::{self, read_u32, write_u32};
use crate::cpu::x86::X87State;

pub const X87_SIZE: usize = 128;
pub const X87_CONTROL_OFFSET: usize = 16;
pub const X87_STATUS_OFFSET: usize = 18;
pub const X87_TAG_OFFSET: usize = 20;
pub const X87_OPCODE_OFFSET: usize = 22;
pub const X87_INSTRUCTION_POINTER_OFFSET: usize = 24;
pub const X87_DATA_POINTER_OFFSET: usize = 28;
pub const X87_CODE_SELECTOR_OFFSET: usize = 32;
pub const X87_DATA_SELECTOR_OFFSET: usize = 34;
pub const X87_REGISTERS_OFFSET: usize = 40;

pub fn encode_x87(state: &X87State, output: &mut [u8]) -> Result<(), AbiError> {
    if output.len() != X87_SIZE {
        return Err(AbiError::Length);
    }
    if state.opcode > 0x7ff {
        return Err(AbiError::Reserved);
    }
    header::write(output, *b"R3FP");
    for (offset, value) in [
        (X87_CONTROL_OFFSET, state.control),
        (X87_STATUS_OFFSET, state.status),
        (X87_TAG_OFFSET, state.tag),
        (X87_OPCODE_OFFSET, state.opcode),
        (X87_CODE_SELECTOR_OFFSET, state.code_selector),
        (X87_DATA_SELECTOR_OFFSET, state.data_selector),
    ] {
        output[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    write_u32(
        output,
        X87_INSTRUCTION_POINTER_OFFSET,
        state.instruction_pointer,
    );
    write_u32(output, X87_DATA_POINTER_OFFSET, state.data_pointer);
    for (index, value) in state.registers.iter().enumerate() {
        let start = X87_REGISTERS_OFFSET + index * 10;
        output[start..start + 10].copy_from_slice(value);
    }
    Ok(())
}

pub fn decode_x87(input: &[u8]) -> Result<X87State, AbiError> {
    header::validate(input, *b"R3FP", X87_SIZE)?;
    if input[36..40]
        .iter()
        .chain(&input[120..128])
        .any(|byte| *byte != 0)
        || read_u16(input, X87_OPCODE_OFFSET) > 0x7ff
    {
        return Err(AbiError::Reserved);
    }
    Ok(X87State {
        control: read_u16(input, X87_CONTROL_OFFSET),
        status: read_u16(input, X87_STATUS_OFFSET),
        tag: read_u16(input, X87_TAG_OFFSET),
        opcode: read_u16(input, X87_OPCODE_OFFSET),
        instruction_pointer: read_u32(input, X87_INSTRUCTION_POINTER_OFFSET),
        data_pointer: read_u32(input, X87_DATA_POINTER_OFFSET),
        code_selector: read_u16(input, X87_CODE_SELECTOR_OFFSET),
        data_selector: read_u16(input, X87_DATA_SELECTOR_OFFSET),
        registers: std::array::from_fn(|index| {
            let start = X87_REGISTERS_OFFSET + index * 10;
            input[start..start + 10].try_into().unwrap()
        }),
    })
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap())
}
