use super::{legacy, validate_mxcsr};
use crate::cpu::x86::{
    X87State,
    simd::{SimdError, SimdState},
};

pub fn save_extended(x87: &X87State, simd: &SimdState, output: &mut [u8; 512]) {
    output[..464].fill(0);
    legacy::save_x87(x87, output);
    legacy::save_simd(simd, output);
}

pub fn restore_extended(input: &[u8; 512]) -> Result<(X87State, SimdState), SimdError> {
    let mxcsr = u32::from_le_bytes(input[24..28].try_into().unwrap());
    validate_mxcsr(mxcsr).map_err(|_| SimdError::GeneralProtection(0))?;
    Ok((
        legacy::restore_x87(input),
        SimdState {
            mxcsr,
            xmm: legacy::restore_xmm(input),
        },
    ))
}
