use super::{
    AbiError,
    cpu_helper::{CpuMemoryError, encode_cpu_result},
    header,
    memory_helper::HELPER_SIZE,
};
use crate::{
    cpu::x86::{EventAuthorityKind32, EventDeliveryResult32, EventOrigin32, ExceptionCause32},
    memory::{Access, FaultReason},
};

mod request;
pub use request::{EVENT_REQUEST_SIZE, EventRequest32, decode_event_request};

pub const EXCEPTION_HELPER_VERSION: u16 = 12;

pub fn encode_exception_result(
    origin: EventOrigin32,
    frame_info: u32,
    result: &EventDeliveryResult32,
    output: &mut [u8],
) -> Result<(), AbiError> {
    if output.len() != HELPER_SIZE {
        return Err(AbiError::Length);
    }
    let slot = frame_info & 0xff;
    if !matches!(
        frame_info,
        0 | 2 | 4 | 0x102 | 0x104 | 0x202 | 0x204 | 0x302 | 0x304
    ) || frame_info != 0 && (frame_info & 0x200 != 0) != origin.error_code().is_some()
    {
        return Err(AbiError::MemoryHelper);
    }
    let mut packet = [0; HELPER_SIZE];
    match result {
        EventDeliveryResult32::Entered(entered) => {
            if slot == 0 || entered.frame_info != frame_info || slot == 2 && entered.target > 0xffff
            {
                return Err(AbiError::MemoryHelper);
            }
            encode_cpu_result(slot, entered.target, Ok(()), &mut packet)?;
        }
        EventDeliveryResult32::ArchitecturalSecondary(cause) => {
            let external = u32::from(origin.external());
            let idt_error = (u32::from(origin.vector()) << 3) | 2 | external;
            let error = match *cause {
                ExceptionCause32::GeneralProtection(code) => {
                    let code = code.get();
                    if (slot == 0 && code != idt_error)
                        || (slot != 0 && !selector_code(code, external, false))
                    {
                        return Err(AbiError::MemoryHelper);
                    }
                    CpuMemoryError::GeneralProtection(code)
                }
                ExceptionCause32::SegmentNotPresent(code) => {
                    let code = code.get();
                    if (slot == 0 && code != idt_error)
                        || (slot != 0 && !selector_code(code, external, true))
                    {
                        return Err(AbiError::MemoryHelper);
                    }
                    CpuMemoryError::SegmentNotPresent(code)
                }
                ExceptionCause32::StackSegment(code) if slot != 0 && code.get() == external => {
                    CpuMemoryError::StackSegment(code.get())
                }
                _ => return Err(AbiError::MemoryHelper),
            };
            encode_cpu_result(slot, 0, Err(error), &mut packet)?;
        }
        EventDeliveryResult32::NeedsPagingAuthority { fault, length } => {
            if slot == 0
                || *length != slot
                || fault.access != Access::Write
                || !matches!(
                    fault.reason,
                    FaultReason::Unmapped | FaultReason::Permission
                )
            {
                return Err(AbiError::MemoryHelper);
            }
            encode_cpu_result(
                slot,
                0,
                Err(CpuMemoryError::Memory(crate::memory::MemoryError::Fault(
                    *fault,
                ))),
                &mut packet,
            )?;
        }
        EventDeliveryResult32::NeedsTaskOrPrivilegeAuthority { kind, selector } => {
            let detail = match kind {
                EventAuthorityKind32::Task if slot == 0 => 1,
                EventAuthorityKind32::Privilege if slot != 0 => 2,
                _ => return Err(AbiError::MemoryHelper),
            };
            header::write_version(&mut packet, *b"R3MH", EXCEPTION_HELPER_VERSION);
            for (offset, value) in [(16, 8), (24, detail), (28, u32::from(*selector))] {
                header::write_u32(&mut packet, offset, value);
            }
        }
        EventDeliveryResult32::Infrastructure(error) => {
            if matches!(error, crate::memory::MemoryError::Fault(_)) {
                return Err(AbiError::MemoryHelper);
            }
            encode_cpu_result(slot, 0, Err(CpuMemoryError::Memory(*error)), &mut packet)?;
        }
    }
    packet[4..6].copy_from_slice(&EXCEPTION_HELPER_VERSION.to_le_bytes());
    header::write_u32(&mut packet, 36, frame_info);
    output.copy_from_slice(&packet);
    Ok(())
}

fn selector_code(code: u32, external: u32, nonnull: bool) -> bool {
    code <= (0xfffc | external) && code & 3 == external && (!nonnull || code & !3 != 0)
}

#[cfg(test)]
mod tests;
