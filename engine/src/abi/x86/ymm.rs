use crate::{
    abi::{AbiError, header},
    cpu::x86::simd::ymm::YmmUpperState,
};

pub const YMM_SIZE: usize = 144;
pub const YMM_REGISTERS_OFFSET: usize = 16;

pub fn encode_ymm(state: &YmmUpperState, output: &mut [u8]) -> Result<(), AbiError> {
    if output.len() != YMM_SIZE {
        return Err(AbiError::Length);
    }
    header::write(output, *b"R3YU");
    for (index, register) in state.registers.iter().enumerate() {
        let offset = YMM_REGISTERS_OFFSET + index * 16;
        output[offset..offset + 16].copy_from_slice(register);
    }
    Ok(())
}

pub fn decode_ymm(input: &[u8]) -> Result<YmmUpperState, AbiError> {
    header::validate(input, *b"R3YU", YMM_SIZE)?;
    Ok(YmmUpperState {
        registers: std::array::from_fn(|index| {
            let offset = YMM_REGISTERS_OFFSET + index * 16;
            input[offset..offset + 16].try_into().unwrap()
        }),
    })
}

#[cfg(test)]
#[path = "ymm/tests.rs"]
mod tests;
