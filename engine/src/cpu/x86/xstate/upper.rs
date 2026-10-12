use super::{XStateError, legacy, validate_mxcsr};
use crate::cpu::x86::{
    X87State,
    simd::{SimdState, ymm::YmmUpperState},
};

pub(crate) mod access;
pub const IMAGE_SIZE: usize = 832;
pub const COMPONENT_OFFSET: usize = 576;
pub const COMPONENT_PAYLOAD: usize = 128;
pub const COMPONENTS: u64 = 7;

pub fn requested(low: u32, high: u32) -> u64 {
    (u64::from(low) | u64::from(high) << 32) & COMPONENTS
}

pub fn validate_header(header: &[u8; 24]) -> Result<u64, XStateError> {
    let present = u64::from_le_bytes(header[..8].try_into().unwrap());
    if present & !COMPONENTS != 0 || header[8..24].iter().any(|byte| *byte != 0) {
        return Err(XStateError);
    }
    Ok(present)
}

/// this dependency codec does not enable guest AVX or alter the public XCR0.
pub fn save(
    request: u64,
    x87: &X87State,
    simd: &SimdState,
    upper: &YmmUpperState,
    image: &mut [u8; IMAGE_SIZE],
) {
    let request = request & COMPONENTS;
    if request & 1 != 0 {
        legacy::save_x87(x87, image);
    }
    if request & 2 != 0 {
        legacy::save_simd(simd, image);
    } else if request & 4 != 0 {
        legacy::save_mxcsr(simd, image);
    }
    if request & 4 != 0 {
        for (index, register) in upper.registers.iter().enumerate() {
            image[576 + index * 16..592 + index * 16].copy_from_slice(register);
        }
    }
    let old = u64::from_le_bytes(image[512..520].try_into().unwrap());
    image[512..520].copy_from_slice(&(old | request).to_le_bytes());
}

pub fn restore(
    request: u64,
    current_x87: &X87State,
    current_simd: &SimdState,
    current_upper: &YmmUpperState,
    image: &[u8; IMAGE_SIZE],
) -> Result<(X87State, SimdState, YmmUpperState), XStateError> {
    let request = request & COMPONENTS;
    let present = validate_header(image[512..536].try_into().unwrap())?;
    let mut x87 = *current_x87;
    let mut simd = *current_simd;
    let mut upper = *current_upper;
    if request & 6 != 0 {
        simd.mxcsr = u32::from_le_bytes(image[24..28].try_into().unwrap());
        validate_mxcsr(simd.mxcsr)?;
    }
    if request & 2 != 0 {
        simd.xmm = if present & 2 != 0 {
            legacy::restore_xmm(image)
        } else {
            [[0; 16]; 8]
        };
    }
    if request & 1 != 0 {
        x87 = if present & 1 != 0 {
            legacy::restore_x87(image)
        } else {
            X87State::default()
        };
    }
    if request & 4 != 0 {
        upper.registers = if present & 4 != 0 {
            std::array::from_fn(|index| {
                image[576 + index * 16..592 + index * 16]
                    .try_into()
                    .unwrap()
            })
        } else {
            [[0; 16]; 8]
        };
    }
    Ok((x87, simd, upper))
}

#[cfg(test)]
#[path = "upper/tests.rs"]
mod tests;
