use super::{MemoryRequirement, MoveSource, VectorRegister, VectorState, VectorWidth};
use crate::{
    math::{DENORMAL, INVALID, OVERFLOW, PRECISION, Rounding, UNDERFLOW, ieee},
    memory::Access,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum F16cTarget {
    Register(VectorRegister),
    Memory,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum F16cInstruction {
    Widen {
        width: VectorWidth,
        destination: VectorRegister,
        source: MoveSource,
    },
    Narrow {
        width: VectorWidth,
        source: VectorRegister,
        destination: F16cTarget,
        immediate: u8,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum F16cError {
    OperandLength,
    InvalidState,
    FloatingPoint(u32),
}

impl F16cInstruction {
    pub(crate) fn memory(self) -> Option<MemoryRequirement> {
        let (access, width) = match self {
            Self::Widen {
                width,
                source: MoveSource::Memory,
                ..
            } => (Access::Read, width),
            Self::Narrow {
                width,
                destination: F16cTarget::Memory,
                ..
            } => (Access::Write, width),
            _ => return None,
        };
        Some(MemoryRequirement {
            access,
            bytes: (width.bytes() / 2) as u8,
            alignment: 1,
        })
    }
}

fn rounding(immediate: u8, mxcsr: u32) -> Rounding {
    let control = if immediate & 4 != 0 {
        (mxcsr >> 13) as u8
    } else {
        immediate
    };
    match control & 3 {
        0 => Rounding::Nearest,
        1 => Rounding::Down,
        2 => Rounding::Up,
        _ => Rounding::Zero,
    }
}

fn signaling_half(bits: u16) -> bool {
    bits & 0x7c00 == 0x7c00 && bits & 0x03ff != 0 && bits & 0x0200 == 0
}

fn signaling_single(bits: u32) -> bool {
    bits & 0x7f80_0000 == 0x7f80_0000 && bits & 0x007f_ffff != 0 && bits & 0x0040_0000 == 0
}

fn denormal_single(bits: u32) -> bool {
    bits & 0x7f80_0000 == 0 && bits & 0x007f_ffff != 0
}

impl VectorState {
    // the operand is an exact checked span; the caller owns segment/page authority
    // and writable-code currentness before invoking this private state kernel.
    pub(crate) fn execute_f16c(
        &mut self,
        operation: F16cInstruction,
        operand: &mut [u8],
    ) -> Result<(), F16cError> {
        let requirement = operation.memory();
        if operand.len() != requirement.map_or(0, |r| usize::from(r.bytes)) {
            return Err(F16cError::OperandLength);
        }
        if self.simd.mxcsr & !0xffff != 0 {
            return Err(F16cError::InvalidState);
        }
        let mxcsr = self.simd.mxcsr;
        let masks = (mxcsr >> 7) & 0x3f;
        let mut result = [0; 32];
        let (destination, exceptions) = match operation {
            F16cInstruction::Widen {
                width,
                destination,
                source,
            } => {
                let input = match source {
                    MoveSource::Register(register) => self.read(register),
                    MoveSource::Memory => {
                        let mut input = [0; 32];
                        input[..operand.len()].copy_from_slice(operand);
                        input
                    }
                };
                let mut exceptions = 0;
                for lane in 0..width.bytes() / 4 {
                    let offset = lane * 2;
                    let bits = u16::from_le_bytes(input[offset..offset + 2].try_into().unwrap());
                    let converted = ieee::half_to_single(bits);
                    result[lane * 4..lane * 4 + 4]
                        .copy_from_slice(&(converted.bits as u32).to_le_bytes());
                    exceptions |= u32::from(signaling_half(bits)) * u32::from(INVALID);
                }
                (F16cTarget::Register(destination), exceptions)
            }
            F16cInstruction::Narrow {
                width,
                source,
                destination,
                immediate,
            } => {
                let input = self.read(source);
                let lanes = width.bytes() / 4;
                let mut prepared = [0; 8];
                let mut pre = 0;
                for (lane, prepared) in prepared.iter_mut().enumerate().take(lanes) {
                    let offset = lane * 4;
                    let mut bits =
                        u32::from_le_bytes(input[offset..offset + 4].try_into().unwrap());
                    pre |= u32::from(signaling_single(bits)) * u32::from(INVALID);
                    if denormal_single(bits) {
                        if mxcsr & 0x40 != 0 {
                            bits &= 0x8000_0000;
                        } else {
                            pre |= u32::from(DENORMAL);
                        }
                    }
                    *prepared = bits;
                }
                if pre & !masks != 0 {
                    self.simd.mxcsr |= pre;
                    return Err(F16cError::FloatingPoint(self.simd.mxcsr));
                }
                let round = rounding(immediate, mxcsr);
                let mut exceptions = pre;
                for (lane, &bits) in prepared.iter().enumerate().take(lanes) {
                    let converted = ieee::single_to_half(bits, round);
                    let mut flags = u32::from(converted.exceptions) & !u32::from(UNDERFLOW);
                    if ieee::half_narrowing_tiny(bits, round)
                        && (masks & u32::from(UNDERFLOW) == 0 || flags & u32::from(PRECISION) != 0)
                    {
                        flags |= u32::from(UNDERFLOW);
                    }
                    if flags & !masks & u32::from(UNDERFLOW | OVERFLOW) != 0 {
                        flags &= !u32::from(PRECISION);
                    }
                    // f16c's source-denormal rule retains all three flags when the
                    // denormal exception is masked and underflow or precision is not.
                    if denormal_single(bits)
                        && masks & u32::from(DENORMAL) != 0
                        && masks & u32::from(UNDERFLOW | PRECISION)
                            != u32::from(UNDERFLOW | PRECISION)
                    {
                        flags |= u32::from(DENORMAL | UNDERFLOW | PRECISION);
                    }
                    exceptions |= flags;
                    result[lane * 2..lane * 2 + 2]
                        .copy_from_slice(&(converted.bits as u16).to_le_bytes());
                }
                (destination, exceptions)
            }
        };
        self.simd.mxcsr |= exceptions;
        if exceptions & !masks != 0 {
            return Err(F16cError::FloatingPoint(self.simd.mxcsr));
        }
        match destination {
            F16cTarget::Register(register) => self.publish(register, result),
            F16cTarget::Memory => operand.copy_from_slice(&result[..operand.len()]),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
