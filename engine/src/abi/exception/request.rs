use crate::{
    abi::{AbiError, header, x86::USER_FLAGS_MASK_V3},
    cpu::x86::{
        DebugCause32, EventOrigin32, ExceptionCause32, FrameInput32, SelectorErrorCode32,
        TrapPoint32,
    },
};

pub const EVENT_REQUEST_SIZE: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventRequest32 {
    pub origin: EventOrigin32,
    pub input: FrameInput32,
}

/// decode one explicit trusted event request; a host exit alone cannot certify its origin.
pub fn decode_event_request(bytes: &[u8]) -> Result<EventRequest32, AbiError> {
    header::validate(bytes, *b"R3EV", EVENT_REQUEST_SIZE)?;
    if bytes[24..32]
        .iter()
        .chain(&bytes[48..64])
        .any(|byte| *byte != 0)
    {
        return Err(AbiError::Reserved);
    }
    let tag = header::read_u32(bytes, 16);
    let detail = header::read_u32(bytes, 20);
    let selector_code = || SelectorErrorCode32::from_bits(detail).ok_or(AbiError::MemoryHelper);
    let origin = match tag {
        0 => {
            let vector = u8::try_from(detail).map_err(|_| AbiError::MemoryHelper)?;
            EventOrigin32::SoftwareInt { vector }
        }
        1..=3 | 9 | 10 if detail == 0 => EventOrigin32::Exception(match tag {
            1 => ExceptionCause32::DivideError,
            2 => ExceptionCause32::BoundRange,
            3 => ExceptionCause32::InvalidOpcode,
            9 => ExceptionCause32::BreakpointInt3,
            10 => ExceptionCause32::OverflowInto,
            _ => unreachable!(),
        }),
        4 => EventOrigin32::Exception(ExceptionCause32::FloatingPoint {
            status: u16::try_from(detail).map_err(|_| AbiError::MemoryHelper)?,
        }),
        5 if detail <= 0xffff => {
            EventOrigin32::Exception(ExceptionCause32::SimdFloatingPoint { mxcsr: detail })
        }
        6 => EventOrigin32::Exception(ExceptionCause32::GeneralProtection(selector_code()?)),
        7 => EventOrigin32::Exception(ExceptionCause32::SegmentNotPresent(selector_code()?)),
        8 => EventOrigin32::Exception(ExceptionCause32::StackSegment(selector_code()?)),
        11 => EventOrigin32::Exception(ExceptionCause32::Debug(match detail {
            0 => DebugCause32::Int1,
            1 => DebugCause32::SingleStep(TrapPoint32::CompletedInstruction),
            2 => DebugCause32::SingleStep(TrapPoint32::RepetitionInProgress),
            3 => DebugCause32::DataBreakpoint(TrapPoint32::CompletedInstruction),
            4 => DebugCause32::DataBreakpoint(TrapPoint32::RepetitionInProgress),
            5 => DebugCause32::IoBreakpoint(TrapPoint32::CompletedInstruction),
            6 => DebugCause32::IoBreakpoint(TrapPoint32::RepetitionInProgress),
            7 => DebugCause32::InstructionBreakpointFault,
            8 => DebugCause32::GeneralDetectFault,
            9 => DebugCause32::TaskSwitchTrap,
            _ => return Err(AbiError::MemoryHelper),
        })),
        // page-fault requests require a detection owner and cr2 commit, not invented rwx metadata.
        _ => return Err(AbiError::MemoryHelper),
    };
    let flags = header::read_u32(bytes, 40);
    if flags & 2 == 0 || flags & !USER_FLAGS_MASK_V3 != 0 {
        return Err(AbiError::Flags);
    }
    Ok(EventRequest32 {
        origin,
        input: FrameInput32 {
            eip: header::read_u32(bytes, 32),
            esp: header::read_u32(bytes, 36),
            flags,
            cs: u16::try_from(header::read_u32(bytes, 44)).map_err(|_| AbiError::MemoryHelper)?,
        },
    })
}

#[cfg(test)]
mod tests;
