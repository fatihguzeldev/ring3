use crate::abi::AbiError;
use crate::abi::header::{self, read_u32, write_u32};
use crate::cpu::x86::State32;

pub const STATE_SIZE: usize = 56;
pub const REGISTERS_OFFSET: usize = 16;
pub const EIP_OFFSET: usize = 48;
pub const EFLAGS_OFFSET: usize = 52;

pub fn encode_state(state: &State32, output: &mut [u8]) -> Result<(), AbiError> {
    if output.len() != STATE_SIZE {
        return Err(AbiError::Length);
    }
    validate_flags(state.eflags)?;
    header::write(output, *b"R3ST");
    for (index, value) in state.registers.iter().enumerate() {
        write_u32(output, REGISTERS_OFFSET + index * 4, *value);
    }
    write_u32(output, EIP_OFFSET, state.eip);
    write_u32(output, EFLAGS_OFFSET, state.eflags);
    Ok(())
}

pub fn decode_state(input: &[u8]) -> Result<State32, AbiError> {
    header::validate(input, *b"R3ST", STATE_SIZE)?;
    let eflags = read_u32(input, EFLAGS_OFFSET);
    validate_flags(eflags)?;
    Ok(State32 {
        registers: std::array::from_fn(|index| read_u32(input, REGISTERS_OFFSET + index * 4)),
        eip: read_u32(input, EIP_OFFSET),
        eflags,
    })
}

fn validate_flags(flags: u32) -> Result<(), AbiError> {
    if flags & 2 == 0 || flags & !0xcd7 != 0 {
        return Err(AbiError::Flags);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state_has_a_valid_binary_roundtrip() {
        let state = State32::default();
        let mut bytes = [0; STATE_SIZE];
        encode_state(&state, &mut bytes).unwrap();
        assert_eq!(decode_state(&bytes).unwrap(), state);
    }
}
