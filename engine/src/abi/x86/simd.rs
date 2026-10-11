use crate::{
    abi::{AbiError, header},
    cpu::x86::simd::SimdState,
};

pub const SIMD_SIZE: usize = 160;
pub const SIMD_MXCSR_OFFSET: usize = 16;
pub const SIMD_XMM_OFFSET: usize = 32;
pub const MXCSR_MASK: u32 = 0x0000_ffff;

pub fn encode_simd(state: &SimdState, output: &mut [u8]) -> Result<(), AbiError> {
    if output.len() != SIMD_SIZE {
        return Err(AbiError::Length);
    }
    if state.mxcsr & !MXCSR_MASK != 0 {
        return Err(AbiError::Reserved);
    }
    header::write(output, *b"R3SV");
    header::write_u32(output, SIMD_MXCSR_OFFSET, state.mxcsr);
    for (index, value) in state.xmm.iter().enumerate() {
        let start = SIMD_XMM_OFFSET + index * 16;
        output[start..start + 16].copy_from_slice(value);
    }
    Ok(())
}

pub fn decode_simd(input: &[u8]) -> Result<SimdState, AbiError> {
    header::validate(input, *b"R3SV", SIMD_SIZE)?;
    let mxcsr = header::read_u32(input, SIMD_MXCSR_OFFSET);
    if mxcsr & !MXCSR_MASK != 0 || input[20..32].iter().any(|byte| *byte != 0) {
        return Err(AbiError::Reserved);
    }
    Ok(SimdState {
        mxcsr,
        xmm: std::array::from_fn(|index| {
            let start = SIMD_XMM_OFFSET + index * 16;
            input[start..start + 16].try_into().unwrap()
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn simd_state_round_trips_every_bit_and_defaults_mxcsr() {
        let state = SimdState {
            mxcsr: 0xffff,
            xmm: std::array::from_fn(|r| std::array::from_fn(|b| (r * 19 + b * 13) as u8)),
        };
        let mut bytes = [0xa5; SIMD_SIZE];
        encode_simd(&state, &mut bytes).unwrap();
        assert_eq!(&bytes[..4], b"R3SV");
        assert_eq!(decode_simd(&bytes), Ok(state));
        assert_eq!(SimdState::default().mxcsr, 0x1f80);
        assert_eq!(&bytes[20..32], &[0; 12]);
    }
    #[test]
    fn malformed_state_is_rejected_without_output_mutation() {
        let mut bytes = [0xa5; SIMD_SIZE];
        let before = bytes;
        let invalid = SimdState {
            mxcsr: 0x10000,
            ..SimdState::default()
        };
        assert_eq!(encode_simd(&invalid, &mut bytes), Err(AbiError::Reserved));
        assert_eq!(bytes, before);
        encode_simd(&SimdState::default(), &mut bytes).unwrap();
        bytes[24] = 1;
        assert_eq!(decode_simd(&bytes), Err(AbiError::Reserved));
        bytes[24] = 0;
        bytes[18] = 1;
        assert_eq!(decode_simd(&bytes), Err(AbiError::Reserved));
        assert_eq!(
            encode_simd(&SimdState::default(), &mut bytes[..159]),
            Err(AbiError::Length)
        );
    }
}
