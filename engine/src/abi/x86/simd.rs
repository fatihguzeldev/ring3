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
    output[SIMD_XMM_OFFSET..SIMD_XMM_OFFSET + 128].copy_from_slice(state.xmm.as_flattened());
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
        xmm: input[SIMD_XMM_OFFSET..SIMD_XMM_OFFSET + 128]
            .as_chunks::<16>()
            .0
            .try_into()
            .unwrap(),
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
        assert_eq!(
            &bytes[32..160],
            &[
                0x00, 0x0d, 0x1a, 0x27, 0x34, 0x41, 0x4e, 0x5b, 0x68, 0x75, 0x82, 0x8f, 0x9c, 0xa9,
                0xb6, 0xc3, 0x13, 0x20, 0x2d, 0x3a, 0x47, 0x54, 0x61, 0x6e, 0x7b, 0x88, 0x95, 0xa2,
                0xaf, 0xbc, 0xc9, 0xd6, 0x26, 0x33, 0x40, 0x4d, 0x5a, 0x67, 0x74, 0x81, 0x8e, 0x9b,
                0xa8, 0xb5, 0xc2, 0xcf, 0xdc, 0xe9, 0x39, 0x46, 0x53, 0x60, 0x6d, 0x7a, 0x87, 0x94,
                0xa1, 0xae, 0xbb, 0xc8, 0xd5, 0xe2, 0xef, 0xfc, 0x4c, 0x59, 0x66, 0x73, 0x80, 0x8d,
                0x9a, 0xa7, 0xb4, 0xc1, 0xce, 0xdb, 0xe8, 0xf5, 0x02, 0x0f, 0x5f, 0x6c, 0x79, 0x86,
                0x93, 0xa0, 0xad, 0xba, 0xc7, 0xd4, 0xe1, 0xee, 0xfb, 0x08, 0x15, 0x22, 0x72, 0x7f,
                0x8c, 0x99, 0xa6, 0xb3, 0xc0, 0xcd, 0xda, 0xe7, 0xf4, 0x01, 0x0e, 0x1b, 0x28, 0x35,
                0x85, 0x92, 0x9f, 0xac, 0xb9, 0xc6, 0xd3, 0xe0, 0xed, 0xfa, 0x07, 0x14, 0x21, 0x2e,
                0x3b, 0x48,
            ]
        );
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
