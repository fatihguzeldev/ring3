use super::{
    AbiError,
    cpu_helper::{CpuMemoryError, encode_cpu_result},
    header,
    memory_helper::HELPER_SIZE,
};
use crate::memory::{Access, FaultReason, MemoryError};

pub const INTERRUPT_HELPER_VERSION: u16 = 11;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InterruptHelperError {
    Cpu(CpuMemoryError),
    Unsupported,
}

pub(crate) fn encode_interrupt_result(
    vector: u8,
    frame_info: u32,
    target: u32,
    result: Result<(), InterruptHelperError>,
    output: &mut [u8],
) -> Result<(), AbiError> {
    if output.len() != HELPER_SIZE {
        return Err(AbiError::Length);
    }
    if !matches!(frame_info, 0 | 2 | 4 | 0x102 | 0x104) {
        return Err(AbiError::MemoryHelper);
    }
    let slot = frame_info & 0xff;
    let mut packet = [0; HELPER_SIZE];
    match result {
        Ok(()) => {
            if slot == 0 || slot == 2 && target > 0xffff {
                return Err(AbiError::MemoryHelper);
            }
            encode_cpu_result(slot, target, Ok(()), &mut packet)?;
        }
        Err(InterruptHelperError::Unsupported) => {
            if frame_info != 0 {
                return Err(AbiError::MemoryHelper);
            }
            header::write_version(&mut packet, *b"R3MH", INTERRUPT_HELPER_VERSION);
            header::write_u32(&mut packet, 16, 8);
            header::write_u32(&mut packet, 24, 4);
        }
        Err(InterruptHelperError::Cpu(error)) => {
            let idt_error = (u32::from(vector) << 3) | 2;
            let valid = if slot == 0 {
                matches!(
                    error,
                    CpuMemoryError::GeneralProtection(code)
                        | CpuMemoryError::SegmentNotPresent(code) if code == idt_error
                )
            } else {
                match error {
                    CpuMemoryError::GeneralProtection(code) => code <= 0xfffc && code & 3 == 0,
                    CpuMemoryError::SegmentNotPresent(code) => {
                        code != 0 && code <= 0xfffc && code & 3 == 0
                    }
                    CpuMemoryError::StackSegment(code) => code == 0,
                    CpuMemoryError::Memory(MemoryError::Fault(fault)) => {
                        fault.access == Access::Write
                            && matches!(
                                fault.reason,
                                FaultReason::Unmapped | FaultReason::Permission
                            )
                    }
                    CpuMemoryError::Memory(_) => true,
                    CpuMemoryError::FloatingPoint(_) | CpuMemoryError::SimdFloatingPoint(_) => {
                        false
                    }
                }
            };
            if !valid {
                return Err(AbiError::MemoryHelper);
            }
            encode_cpu_result(slot, 0, Err(error), &mut packet)?;
        }
    }
    packet[4..6].copy_from_slice(&INTERRUPT_HELPER_VERSION.to_le_bytes());
    header::write_u32(&mut packet, 36, frame_info);
    output.copy_from_slice(&packet);
    Ok(())
}

#[cfg(test)]
mod tests;
