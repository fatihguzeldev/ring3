use super::{DetectedEvent32, InstructionTrapRestart32};
use crate::{
    cpu::SystemEntryKind32,
    cpu::x86::{DebugCause32, EventOrigin32, ExceptionCause32, SelectorErrorCode32, TrapPoint32},
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
    process::{EngineInstance, HostError},
};

impl EngineInstance {
    pub(in crate::process) fn finish_generated_detection(
        &mut self,
        epoch: u64,
        fields: [u32; 11],
    ) -> Result<(), HostError> {
        #[cfg(any(target_arch = "wasm32", test))]
        let progress = self.prepare_execution_progress()?;
        if fields[0] == 3 {
            if fields[1..] != [0; 10] {
                return Err(HostError::InvalidRequest);
            }
            self.finish_need_code_detection(epoch)?;
        } else {
            let (event, restart) =
                generated_site_with_paging(fields, self.certified_page_fault32(epoch))?;
            self.finish_exception_detection(epoch, event, restart)?;
            self.cpu_event = Some(self.consume_exception_detection(epoch)?.into_event());
        }
        #[cfg(any(target_arch = "wasm32", test))]
        self.commit_execution_progress(progress);
        #[cfg(any(target_arch = "wasm32", test))]
        if fields[0] == 3 {
            self.record_cache_need_code(epoch);
        }
        #[cfg(any(all(feature = "observability", target_arch = "wasm32"), test))]
        self.sealed_execution_census_child(epoch);
        Ok(())
    }
}

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
    let event = if reason == 24 {
        if !matches!(detail, 0x29 | 0x2e | 0x100) || address != 0 || access != 0 || length != 0 {
            return Err(invalid);
        }
        DetectedEvent32::Exception(EventOrigin32::SystemEntry(if detail == 0x100 {
            SystemEntryKind32::Sysenter
        } else {
            SystemEntryKind32::Interrupt(detail as u8)
        }))
    } else if reason == 5 {
        DetectedEvent32::MemoryFailure {
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
            22 if detail == 0 => ExceptionCause32::DeviceNotAvailable,
            _ => return Err(invalid),
        };
        DetectedEvent32::Exception(EventOrigin32::Exception(cause))
    };
    let restart = if matches!(reason, 17 | 18 | 24) || reason == 16 && debug == 1 {
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

pub(in crate::process) fn generated_site_with_paging(
    fields: [u32; 11],
    witness: Option<crate::cpu::x86::VerifiedPageFault32>,
) -> Result<(DetectedEvent32, Option<InstructionTrapRestart32>), HostError> {
    if fields[0] != 23 {
        return generated_site(fields);
    }
    let fault = witness.ok_or(HostError::InvalidRequest)?;
    let access = match fault.access() {
        crate::cpu::x86::PagingAccess32::Read => 1,
        crate::cpu::x86::PagingAccess32::Write => 2,
        crate::cpu::x86::PagingAccess32::Execute => 3,
    };
    let max_length = if access == 3 { 15 } else { 832 };
    if fields[1] != fault.error_code()
        || fields[2] != fault.linear().0
        || fields[3] != access
        || !(1..=max_length).contains(&fields[4])
        || fields[5..] != [0; 6]
    {
        return Err(HostError::InvalidRequest);
    }
    Ok((
        DetectedEvent32::Exception(EventOrigin32::Exception(ExceptionCause32::PageFault(fault))),
        None,
    ))
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
    fn raw_memory_and_null_task_return_sites_keep_precise_fault_fields() {
        let (event, restart) = generated_site([5, 2, 0x5000, 2, 4, 0, 0, 0, 0, 0, 0]).unwrap();
        assert!(matches!(
            event,
            DetectedEvent32::MemoryFailure { length: 4, .. }
        ));
        assert!(restart.is_none());
        assert!(generated_site([21, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]).is_ok());
        assert!(generated_site([21, 0x4e, 0, 0, 0, 0, 0, 0, 0, 0, 0]).is_err());
        assert!(generated_site([20, 0, 0x5000, 0, 0, 0, 0, 0, 0, 0, 0]).is_err());
    }

    #[test]
    fn paging_site_requires_the_actual_epoch_witness_and_exact_metadata() {
        use crate::cpu::x86::{PagingAccess32, VerifiedPageFault32};
        let fault =
            VerifiedPageFault32::application(GuestAddress(0x3005), PagingAccess32::Read, false);
        let fields = [23, 4, 0x3005, 1, 4, 0, 0, 0, 0, 0, 0];
        assert!(generated_site_with_paging(fields, None).is_err());
        assert_eq!(
            generated_site_with_paging(fields, Some(fault)),
            Ok((
                DetectedEvent32::Exception(EventOrigin32::Exception(ExceptionCause32::PageFault(
                    fault
                ))),
                None
            ))
        );
        for (index, value) in [
            (1, 5),
            (2, 0x3004),
            (3, 2),
            (4, 0),
            (4, 833),
            (5, 1),
            (6, 0x1000),
            (10, 1),
        ] {
            let mut changed = fields;
            changed[index] = value;
            assert!(generated_site_with_paging(changed, Some(fault)).is_err());
        }
        assert!(generated_site(fields).is_err());
    }
}

#[cfg(test)]
mod system_entry_tests {
    use super::*;
    #[test]
    fn os_entry_requires_known_kind_empty_fault_geometry_and_trusted_restart_shape() {
        for (detail, kind) in [
            (0x29, SystemEntryKind32::Interrupt(0x29)),
            (0x2e, SystemEntryKind32::Interrupt(0x2e)),
            (0x100, SystemEntryKind32::Sysenter),
        ] {
            let fields = [24, detail, 0, 0, 0, 0, 0x1000, 7, 0x10002, 3, 11];
            let (event, restart) = generated_site(fields).unwrap();
            assert_eq!(
                event,
                DetectedEvent32::Exception(EventOrigin32::SystemEntry(kind))
            );
            assert_eq!(restart.unwrap().eip, 0x1000);
            assert_eq!(restart.unwrap().work_consumed, Some(11));
            for index in [2, 3, 4, 5] {
                let mut invalid = fields;
                invalid[index] = 1;
                assert!(generated_site(invalid).is_err());
            }
            let mut invalid = fields;
            invalid[9] = 4;
            assert!(generated_site(invalid).is_err());
        }
        for vector in [0, 3, 4, 0x80, 0x129, u32::MAX] {
            assert!(generated_site([24, vector, 0, 0, 0, 0, 0x1000, 0, 2, 0, 0]).is_err());
        }
    }
}
