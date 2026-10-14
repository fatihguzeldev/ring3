use super::{DetectedEvent32, InstructionTrapRestart32};
use crate::{
    cpu::x86::{DebugCause32, EventOrigin32, ExceptionCause32, SelectorErrorCode32, TrapPoint32},
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
    process::HostError,
};

/// inputs are the terminal generated locals, independently checked against the flushed snapshot.
pub(in crate::process) fn generated_site(
    fields: [u32; 11],
) -> Result<(DetectedEvent32, Option<InstructionTrapRestart32>), HostError> {
    let [
        reason,
        detail,
        address,
        access,
        length,
        debug,
        pc,
        retired,
        flags,
        shadow,
        work,
    ] = fields;
    let invalid = HostError::InvalidRequest;
    if reason != 16 && debug != 0 {
        return Err(invalid);
    }
    let event = if reason == 5 {
        DetectedEvent32::NeedsPagingAuthority {
            fault: MemoryFault {
                address: GuestAddress(address),
                access: match access {
                    1 => Access::Read,
                    2 => Access::Write,
                    3 => Access::Execute,
                    _ => return Err(invalid),
                },
                reason: match detail {
                    1 => FaultReason::Unmapped,
                    2 => FaultReason::Permission,
                    3 => FaultReason::AddressOverflow,
                    _ => return Err(invalid),
                },
            },
            length,
        }
    } else {
        if address != 0 || access != 0 || length != 0 {
            return Err(invalid);
        }
        let selector = || SelectorErrorCode32::from_bits(detail).ok_or(invalid);
        let cause = match reason {
            10 if detail == 0 => ExceptionCause32::DivideError,
            11 => ExceptionCause32::GeneralProtection(selector()?),
            12 if detail <= u16::MAX as u32 => ExceptionCause32::FloatingPoint {
                status: detail as u16,
            },
            13 if detail <= u16::MAX as u32 => {
                ExceptionCause32::SimdFloatingPoint { mxcsr: detail }
            }
            14 => ExceptionCause32::SegmentNotPresent(selector()?),
            15 => ExceptionCause32::StackSegment(selector()?),
            16 if detail == 0 => ExceptionCause32::Debug(match debug {
                1 => DebugCause32::Int1,
                2 => DebugCause32::SingleStep(TrapPoint32::CompletedInstruction),
                3 => DebugCause32::SingleStep(TrapPoint32::RepetitionInProgress),
                _ => return Err(invalid),
            }),
            17 if detail == 0 => ExceptionCause32::BreakpointInt3,
            18 if detail == 0 => ExceptionCause32::OverflowInto,
            19 if detail == 0 => ExceptionCause32::BoundRange,
            20 if detail == 0 => ExceptionCause32::InvalidOpcode,
            21 if detail <= 0xfffd && detail & 2 == 0 => ExceptionCause32::InvalidTss(selector()?),
            _ => return Err(invalid),
        };
        DetectedEvent32::Exception(EventOrigin32::Exception(cause))
    };
    let restart = if matches!(reason, 17 | 18) || reason == 16 && debug == 1 {
        if shadow & !3 != 0 || shadow & 2 == 0 && work != 0 {
            return Err(invalid);
        }
        Some(InstructionTrapRestart32 {
            eip: pc,
            retired,
            flags,
            ss_debug_shadow: shadow & 1 != 0,
            work_consumed: (shadow & 2 != 0).then_some(work),
        })
    } else {
        if [pc, retired, flags, shadow, work] != [0; 5] {
            return Err(invalid);
        }
        None
    };
    Ok((event, restart))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_site_is_required_and_restart_is_canonical() {
        for debug in [1, 2, 3] {
            let mut fields = [16, 0, 0, 0, 0, debug, 0, 0, 0, 0, 0];
            if debug == 1 {
                fields[6] = 0x1000;
                fields[8] = 2;
            }
            assert!(generated_site(fields).is_ok());
        }
        assert!(generated_site([16, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]).is_err());
        assert!(generated_site([20, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0]).is_err());
        assert!(generated_site([20, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0]).is_err());
        assert!(generated_site([17, 0, 0, 0, 0, 0, 0x1000, 0, 2, 4, 0]).is_err());
        assert!(generated_site([17, 0, 0, 0, 0, 0, 0x1000, 0, 2, 0, 1]).is_err());
    }

    #[test]
    fn raw_memory_and_tss_sites_keep_their_authority_and_selector_rules() {
        let (event, restart) = generated_site([5, 2, 0x5000, 2, 4, 0, 0, 0, 0, 0, 0]).unwrap();
        assert!(matches!(
            event,
            DetectedEvent32::NeedsPagingAuthority { length: 4, .. }
        ));
        assert!(restart.is_none());
        assert!(generated_site([21, 0x4d, 0, 0, 0, 0, 0, 0, 0, 0, 0]).is_ok());
        assert!(generated_site([21, 0x4e, 0, 0, 0, 0, 0, 0, 0, 0, 0]).is_err());
        assert!(generated_site([20, 0, 0x5000, 0, 0, 0, 0, 0, 0, 0, 0]).is_err());
    }
}
