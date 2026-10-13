use super::{X87State, ir::EffectiveAddress, simd::SimdState};

pub(crate) mod access;
pub(in crate::cpu::x86) mod legacy;
pub mod upper;

pub const IMAGE_SIZE: usize = 576;

/// the original helper protocol owns only x87 and xmm components, even when xcr0 grows.
const COMPONENTS: u64 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XStateOperation {
    Save { address: EffectiveAddress },
    Restore { address: EffectiveAddress },
    ReadControl,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct XStateError;

pub fn read_control(index: u32) -> Result<u64, XStateError> {
    if index == 0 {
        Ok(super::capabilities::XCR0)
    } else {
        Err(XStateError)
    }
}

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

pub fn validate_mxcsr(mxcsr: u32) -> Result<(), XStateError> {
    if mxcsr & !0xffff != 0 {
        Err(XStateError)
    } else {
        Ok(())
    }
}

/// standard-format component codecs preserve every unrequested byte.
pub fn save(request: u64, x87: &X87State, simd: &SimdState, image: &mut [u8; IMAGE_SIZE]) {
    let request = request & COMPONENTS;
    if request & 1 != 0 {
        legacy::save_x87(x87, image);
    }
    if request & 2 != 0 {
        legacy::save_simd(simd, image);
    }
    let old = u64::from_le_bytes(image[512..520].try_into().unwrap());
    // this profile has no init optimization and conservatively maintains xinuse=3.
    image[512..520].copy_from_slice(&(old | request).to_le_bytes());
}

pub fn restore(
    request: u64,
    current_x87: &X87State,
    current_simd: &SimdState,
    image: &[u8; IMAGE_SIZE],
) -> Result<(X87State, SimdState), XStateError> {
    let request = request & COMPONENTS;
    let present = validate_header(image[512..536].try_into().unwrap())?;
    let mut x87 = *current_x87;
    let mut simd = *current_simd;
    if request & 2 != 0 {
        simd.mxcsr = u32::from_le_bytes(image[24..28].try_into().unwrap());
        validate_mxcsr(simd.mxcsr)?;
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
    Ok((x87, simd))
}

#[cfg(test)]
#[path = "xstate/native.rs"]
mod native;
