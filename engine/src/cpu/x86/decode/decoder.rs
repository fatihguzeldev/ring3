use super::{
    BoundDecode, CapturedCodeFault, CapturedFetchFaultKind32, CodeFetchContext, DecodeAuthority,
    DecodedInstruction, DecodedOperation,
};
use iced_x86::{Decoder, DecoderError, DecoderOptions};

use crate::{
    cpu::{
        UnsupportedFeature,
        x86::{CpuPagingError32, environment::USER_MODE_ENVIRONMENT, ir::Operation},
    },
    memory::{AddressSpace, CodeSnapshot, GuestAddress, MemoryError, MemoryFault},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    MemoryFault {
        pc: GuestAddress,
        fault: MemoryFault,
        length: u32,
    },
    CodeSegment {
        pc: GuestAddress,
        attempted_length: u8,
    },
    Paging {
        pc: GuestAddress,
        error: CpuPagingError32,
        attempted_length: u8,
    },
    MissingSystemRegisterAuthority(crate::cpu::x86::SystemRegisterQuery),
    InvalidEncoding,
    Unsupported(UnsupportedFeature),
    Infrastructure(MemoryError),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum DecodePolicy {
    Strict,
    Execution,
}

pub fn decode_one(
    memory: &AddressSpace,
    pc: GuestAddress,
) -> Result<DecodedInstruction, DecodeError> {
    #[cfg(feature = "observability")]
    let _scope = ring3_observability::current().scope(
        ring3_observability::Level::Detailed,
        "engine.cpu.decode",
        "instruction",
        &[ring3_observability::Field::unsigned("pc", u64::from(pc.0))],
    );
    unbound(snapshot_decode(
        memory,
        decode_core(
            memory,
            pc,
            DecodePolicy::Strict,
            None,
            DecodeAuthority::default(),
        )?,
        None,
    )?)
}

#[cfg(test)]
pub(crate) fn decode_for_execution(
    memory: &AddressSpace,
    pc: GuestAddress,
) -> Result<DecodedInstruction, DecodeError> {
    unbound(decode(
        memory,
        pc,
        DecodePolicy::Execution,
        None,
        DecodeAuthority::default(),
    )?)
}

fn unbound<I>(decoded: BoundDecode<I>) -> Result<I, DecodeError> {
    match decoded {
        BoundDecode::Instruction(instruction) => Ok(instruction),
        BoundDecode::CodeFault(_) => unreachable!("unbound decoding has no CS authority"),
    }
}

#[cfg(test)]
pub(crate) fn decode_bound(
    memory: &AddressSpace,
    pc: GuestAddress,
    cs: CodeFetchContext,
    policy: DecodePolicy,
) -> Result<BoundDecode<DecodedInstruction>, DecodeError> {
    decode(memory, pc, policy, Some(cs), DecodeAuthority::default())
}

#[cfg(test)]
pub(crate) fn decode_with_authority(
    memory: &AddressSpace,
    pc: GuestAddress,
    policy: DecodePolicy,
    cs: Option<CodeFetchContext>,
    authority: DecodeAuthority,
) -> Result<BoundDecode<DecodedInstruction>, DecodeError> {
    decode(memory, pc, policy, cs, authority)
}

pub(crate) fn decode_for_compilation(
    memory: &AddressSpace,
    pc: GuestAddress,
    policy: DecodePolicy,
    cs: Option<CodeFetchContext>,
    authority: DecodeAuthority,
) -> Result<BoundDecode, DecodeError> {
    decode_core(memory, pc, policy, cs, authority)
}

#[cfg(test)]
fn decode(
    memory: &AddressSpace,
    pc: GuestAddress,
    policy: DecodePolicy,
    cs: Option<CodeFetchContext>,
    authority: DecodeAuthority,
) -> Result<BoundDecode<DecodedInstruction>, DecodeError> {
    snapshot_decode(memory, decode_core(memory, pc, policy, cs, authority)?, cs)
}

fn snapshot_decode(
    memory: &AddressSpace,
    decoded: BoundDecode,
    cs: Option<CodeFetchContext>,
) -> Result<BoundDecode<DecodedInstruction>, DecodeError> {
    Ok(match decoded {
        BoundDecode::Instruction(instruction) => {
            let pc = instruction.pc();
            let length = usize::from(instruction.length());
            let code_snapshot = code_snapshot(memory, pc, length, cs)
                .map_err(|error| memory_error(error, pc, length))?;
            BoundDecode::Instruction(DecodedInstruction::new(instruction, code_snapshot))
        }
        BoundDecode::CodeFault(fault) => BoundDecode::CodeFault(fault),
    })
}

fn decode_core(
    memory: &AddressSpace,
    pc: GuestAddress,
    policy: DecodePolicy,
    cs: Option<CodeFetchContext>,
    authority: DecodeAuthority,
) -> Result<BoundDecode, DecodeError> {
    let bitness = cs.map_or(32, CodeFetchContext::decoder_bitness);
    let mut bytes = [0; 15];
    for length in 1..=bytes.len() {
        if cs.is_some_and(|cs| !cs.permits(pc, length as u8)) {
            if policy == DecodePolicy::Strict {
                return Err(DecodeError::CodeSegment {
                    pc,
                    attempted_length: length as u8,
                });
            }
            let consumed = length - 1;
            let prefix = if consumed == 0 {
                None
            } else {
                Some(
                    code_snapshot(memory, pc, consumed, cs)
                        .map_err(|error| memory_error(error, pc, consumed))?,
                )
            };
            return Ok(BoundDecode::CodeFault(CapturedCodeFault {
                kind: CapturedFetchFaultKind32::Segment,
                pc,
                attempted_length: length as u8,
                consumed_prefix_length: consumed as u8,
                prefix,
            }));
        }
        let fetched = if let Some(cs) = cs {
            cs.fetch(memory, pc, &mut bytes[..length])
        } else {
            super::fetch_unbound(memory, pc, &mut bytes[..length])
        };
        if let Err(error) = fetched {
            if policy == DecodePolicy::Execution
                && cs.is_some()
                && matches!(&error, CpuPagingError32::PageFault(_))
            {
                let consumed = length - 1;
                let prefix = if consumed == 0 {
                    None
                } else {
                    Some(
                        code_snapshot(memory, pc, consumed, cs)
                            .map_err(|error| memory_error(error, pc, consumed))?,
                    )
                };
                return Ok(BoundDecode::CodeFault(CapturedCodeFault {
                    kind: CapturedFetchFaultKind32::Paging,
                    pc,
                    attempted_length: length as u8,
                    consumed_prefix_length: consumed as u8,
                    prefix,
                }));
            }
            return Err(paging_error(error, pc, length));
        }
        let mut decoder = Decoder::with_ip(
            bitness,
            &bytes[..length],
            u64::from(pc.0),
            DecoderOptions::NONE,
        );
        let instruction = decoder.decode();
        match decoder.last_error() {
            DecoderError::NoMoreBytes => continue,
            DecoderError::InvalidInstruction => {
                if policy == DecodePolicy::Execution
                    && let Some(operation) = super::execution_faults::invalid(&bytes[..length])
                {
                    return Ok(BoundDecode::Instruction(finish(
                        pc,
                        length,
                        GuestAddress(pc.0.wrapping_add(length as u32)),
                        Operation::Control(operation),
                        0,
                        false,
                    )));
                }
                return Err(DecodeError::InvalidEncoding);
            }
            DecoderError::None => {
                let consumed = instruction.len();
                let fault = if policy == DecodePolicy::Execution {
                    super::execution_faults::for_instruction(
                        &instruction,
                        &bytes[..consumed],
                        USER_MODE_ENVIRONMENT,
                    )
                } else {
                    None
                };
                let (operation, segments) = if let Some(operation) = fault {
                    (Operation::Control(operation), 0)
                } else {
                    match super::lower::lower(&instruction, &bytes[..consumed], authority) {
                        Ok(operation) => (operation, segment_accesses(&instruction)),
                        Err(DecodeError::InvalidEncoding)
                            if policy == DecodePolicy::Execution
                                && instruction.encoding() == iced_x86::EncodingKind::VEX =>
                        {
                            (
                                Operation::Control(
                                    crate::cpu::x86::ControlOperation::InvalidOpcode,
                                ),
                                0,
                            )
                        }
                        Err(error) => return Err(error),
                    }
                };
                return Ok(BoundDecode::Instruction(finish(
                    pc,
                    consumed,
                    GuestAddress(instruction.next_ip32()),
                    operation,
                    segments,
                    matches!(
                        fault,
                        Some(
                            crate::cpu::x86::ControlOperation::GeneralProtectionZero
                                | crate::cpu::x86::ControlOperation::DeniedPortString { .. }
                        )
                    ),
                )));
            }
            _ => return Err(DecodeError::InvalidEncoding),
        }
    }
    Err(DecodeError::InvalidEncoding)
}

fn finish(
    pc: GuestAddress,
    length: usize,
    next_pc: GuestAddress,
    operation: Operation,
    segment_accesses: u8,
    privileged_instruction: bool,
) -> DecodedOperation {
    DecodedOperation::new(
        pc,
        length as u8,
        next_pc,
        operation,
        segment_accesses,
        privileged_instruction,
    )
}

fn code_snapshot(
    memory: &AddressSpace,
    pc: GuestAddress,
    length: usize,
    cs: Option<CodeFetchContext>,
) -> Result<CodeSnapshot, MemoryError> {
    if let Some(cs) = cs {
        cs.snapshot(memory, pc, length)
    } else {
        memory.snapshot_code(pc, length)
    }
}

pub(crate) fn paging_error(
    error: CpuPagingError32,
    pc: GuestAddress,
    length: usize,
) -> DecodeError {
    match error {
        CpuPagingError32::Memory(error) => memory_error(error, pc, length),
        error => DecodeError::Paging {
            pc,
            error,
            attempted_length: length as u8,
        },
    }
}

fn memory_error(error: MemoryError, pc: GuestAddress, length: usize) -> DecodeError {
    match error {
        MemoryError::Fault(fault) => DecodeError::MemoryFault {
            pc,
            fault,
            length: length as u32,
        },
        other => DecodeError::Infrastructure(other),
    }
}

#[cfg(test)]
#[path = "decoder/compilation.rs"]
mod compilation_tests;

fn segment_accesses(instruction: &iced_x86::Instruction) -> u8 {
    use iced_x86::{Code, InstructionInfoFactory, Mnemonic, Register};
    // software interrupts stop at the application os boundary without stack accesses.
    if instruction.code() == Code::Int_imm8 {
        return 0;
    }
    if super::profile::ignored_prefetch_alias(instruction)
        || super::profile::ignored_instruction_prefetch(instruction)
        || super::profile::legacy_nop_alias(instruction)
    {
        return 0;
    }
    if crate::cpu::x86::capabilities::PREFETCHW_ENABLED
        && matches!(instruction.code(), Code::Prefetch_m8 | Code::Prefetchw_m8)
    {
        return 0;
    }
    if matches!(
        instruction.mnemonic(),
        Mnemonic::Lea
            | Mnemonic::Nop
            | Mnemonic::Prefetchnta
            | Mnemonic::Prefetcht0
            | Mnemonic::Prefetcht1
            | Mnemonic::Prefetcht2
            | Mnemonic::Cldemote
    ) {
        return 0;
    }
    InstructionInfoFactory::new()
        .info(instruction)
        .used_memory()
        .iter()
        .fold(0, |mask, memory| {
            mask | match memory.segment() {
                Register::ES => 1,
                Register::CS => 2,
                Register::SS => 4,
                Register::DS => 8,
                Register::FS => 16,
                Register::GS => 32,
                _ => 0,
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{PageRange, Permissions};

    #[test]
    fn decodes_immediate_move() {
        let mut memory = AddressSpace::new(1).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        memory
            .write(GuestAddress(0), &[0xb8, 0x78, 0x56, 0x34, 0x12])
            .unwrap();
        let decoded = decode_one(&memory, GuestAddress(0)).unwrap();
        assert_eq!(decoded.length(), 5);
        assert_eq!(decoded.next_pc(), GuestAddress(5));
        assert!(memory.is_code_current(decoded.code_snapshot()));
    }

    fn executable(bytes: &[u8], pc: u32) -> AddressSpace {
        let mut memory = AddressSpace::new(1).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(pc & !0xfff), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        memory.write(GuestAddress(pc), bytes).unwrap();
        memory
    }

    #[test]
    fn ignored_data_prefetch_memory_types_have_no_operand_segment_accesses() {
        for selector in 0..8_u8 {
            for prefix in [
                &[][..],
                &[0x66],
                &[0x67],
                &[0x64],
                &[0x36],
                &[0x2e],
                &[0x66, 0x67],
            ] {
                let bytes = [prefix, &[0x0f, 0x0d, selector << 3]].concat();
                let memory = executable(&bytes, 0x1000);
                let instruction = decode_one(&memory, GuestAddress(0x1000)).unwrap();
                assert_eq!(instruction.segment_accesses(), 0, "{bytes:02x?}");
            }
        }
    }

    #[test]
    fn execution_decode_delivers_only_proven_invalid_or_explicit_cpl3_hlt_faults() {
        use crate::cpu::x86::ControlOperation;
        for (bytes, fault, strict) in [
            (
                &[0xf0, 0x90][..],
                ControlOperation::InvalidOpcode,
                DecodeError::InvalidEncoding,
            ),
            (
                &[0xf0, 0xf4][..],
                ControlOperation::InvalidOpcode,
                DecodeError::InvalidEncoding,
            ),
            (
                &[0x0f, 0x0a, 0][..],
                ControlOperation::InvalidOpcode,
                DecodeError::InvalidEncoding,
            ),
            (
                &[0xf4][..],
                ControlOperation::GeneralProtectionZero,
                DecodeError::Unsupported(UnsupportedFeature::Privileged),
            ),
            (
                &[0x66, 0xf4][..],
                ControlOperation::GeneralProtectionZero,
                DecodeError::Unsupported(UnsupportedFeature::Privileged),
            ),
            (
                &[0x64, 0x67, 0xf4][..],
                ControlOperation::GeneralProtectionZero,
                DecodeError::Unsupported(UnsupportedFeature::Privileged),
            ),
        ] {
            let memory = executable(bytes, 0x1000);
            assert_eq!(
                decode_one(&memory, GuestAddress(0x1000)).unwrap_err(),
                strict
            );
            let instruction = decode_for_execution(&memory, GuestAddress(0x1000)).unwrap();
            assert_eq!(
                *instruction.operation(),
                Operation::Control(fault),
                "{bytes:02x?}"
            );
            assert_eq!(usize::from(instruction.length()), bytes.len());
            assert_eq!(
                instruction.next_pc(),
                GuestAddress(0x1000 + bytes.len() as u32)
            );
            assert_eq!(instruction.segment_accesses(), 0);
            assert!(memory.is_code_current(instruction.code_snapshot()));
        }
    }

    #[test]
    fn execution_decode_keeps_valid_unsupported_and_duplicate_profile_refusals() {
        for bytes in [
            &[0xf2, 0xfa][..],
            &[0xf3, 0xfb][..],
            &[0xf3, 0x0f, 0xa2][..],
            &[0x66, 0x66, 0x90][..],
            &[0x66, 0x66, 0xf4][..],
            &[0xf2, 0xf4][..],
            &[0xf3, 0xf4][..],
        ] {
            let memory = executable(bytes, 0x1000);
            let strict = decode_one(&memory, GuestAddress(0x1000)).unwrap_err();
            assert!(
                matches!(strict, DecodeError::Unsupported(_)),
                "{bytes:02x?}"
            );
            assert_eq!(
                decode_for_execution(&memory, GuestAddress(0x1000)).unwrap_err(),
                strict
            );
        }
    }

    #[test]
    fn execution_decode_proves_prefix_ceiling_without_guessing_other_15_byte_errors() {
        use crate::cpu::x86::ControlOperation;
        let bytes = [0x2e; 15];
        let mut memory = executable(&bytes, 0x1ff1);
        assert_eq!(
            decode_one(&memory, GuestAddress(0x1ff1)).unwrap_err(),
            DecodeError::InvalidEncoding
        );
        let instruction = decode_for_execution(&memory, GuestAddress(0x1ff1)).unwrap();
        assert_eq!(
            *instruction.operation(),
            Operation::Control(ControlOperation::GeneralProtectionZero)
        );
        assert_eq!(instruction.length(), 15);
        assert_eq!(instruction.next_pc(), GuestAddress(0x2000));
        assert!(memory.is_code_current(instruction.code_snapshot()));
        memory.write(GuestAddress(0x1fff), &[0x90]).unwrap();
        assert!(!memory.is_code_current(instruction.code_snapshot()));
        let mut reserved = vec![0x2e; 13];
        reserved.extend([0x0f, 0x0a]);
        let mut needs_16 = vec![0x2e; 11];
        needs_16.extend([0xb8, 1, 2, 3]);
        for bytes in [reserved, needs_16] {
            let memory = executable(&bytes, 0x1ff1);
            assert_eq!(
                decode_for_execution(&memory, GuestAddress(0x1ff1)).unwrap_err(),
                DecodeError::InvalidEncoding,
                "{bytes:02x?}"
            );
        }
    }

    #[test]
    fn execution_decode_preserves_exact_fetch_fault_before_complete_instruction() {
        for (pc, bytes, expected_length) in [
            (0x1fff, &[0x0f][..], 2),
            (0x1ffe, &[0x0f, 0x0a][..], 3),
            (u32::MAX, &[0x0f][..], 2),
        ] {
            let memory = executable(bytes, pc);
            let strict = decode_one(&memory, GuestAddress(pc)).unwrap_err();
            if pc == u32::MAX {
                assert_eq!(
                    strict,
                    DecodeError::MemoryFault {
                        pc: GuestAddress(pc),
                        fault: MemoryFault {
                            address: GuestAddress(pc),
                            access: crate::memory::Access::Execute,
                            reason: crate::memory::FaultReason::AddressOverflow
                        },
                        length: u32::from(expected_length),
                    }
                );
            } else {
                let DecodeError::Paging {
                    pc: fault_pc,
                    attempted_length,
                    error: crate::cpu::x86::CpuPagingError32::PageFault(fault),
                } = strict
                else {
                    panic!("expected exact page fault, got {strict:?}")
                };
                assert_eq!(
                    (fault_pc, attempted_length),
                    (GuestAddress(pc), expected_length)
                );
                assert_eq!(
                    (fault.linear(), fault.access(), fault.error_code()),
                    (
                        GuestAddress(0x2000),
                        crate::cpu::x86::PagingAccess32::Execute,
                        20
                    )
                );
            }
            assert_eq!(
                decode_for_execution(&memory, GuestAddress(pc)).unwrap_err(),
                strict
            );
        }
    }
}

#[cfg(test)]
mod based_tests {
    use super::*;
    use crate::{
        cpu::x86::SegmentContext,
        memory::{PageRange, Permissions},
    };

    #[test]
    fn based_decode_keeps_logical_pc_and_owns_only_linear_instruction_bytes() {
        let mut memory = AddressSpace::new(2).unwrap();
        for page in [0x1000, 0x5000] {
            memory
                .map_zeroed(
                    PageRange::new(GuestAddress(page), 1).unwrap(),
                    Permissions::ALL,
                )
                .unwrap();
        }
        memory.write(GuestAddress(0x1010), &[0x40; 5]).unwrap();
        memory
            .write(GuestAddress(0x5010), &[0xb8, 0x78, 0x56, 0x34, 0x12])
            .unwrap();
        let mut segments = SegmentContext::default();
        segments.state.bases[1] = 0x4000;
        let cs = CodeFetchContext::from_segments(&segments).unwrap();
        let BoundDecode::Instruction(decoded) =
            decode_bound(&memory, GuestAddress(0x1010), cs, DecodePolicy::Strict).unwrap()
        else {
            panic!("instruction expected")
        };
        assert_eq!(
            (decoded.pc(), decoded.next_pc(), decoded.length()),
            (GuestAddress(0x1010), GuestAddress(0x1015), 5)
        );
        memory.write(GuestAddress(0x1010), &[0x48]).unwrap();
        assert!(
            memory.is_code_current(decoded.code_snapshot()),
            "logical decoy page is not captured"
        );
        memory.write(GuestAddress(0x5010), &[0x48]).unwrap();
        assert!(!memory.is_code_current(decoded.code_snapshot()));
    }

    #[test]
    fn based_fetch_accepts_wrapped_base_addition_and_split_linear_instruction() {
        let mut memory = AddressSpace::new(3).unwrap();
        for page in [0, 0x1000, 0xffff_f000] {
            memory
                .map_zeroed(
                    PageRange::new(GuestAddress(page), 1).unwrap(),
                    Permissions::ALL,
                )
                .unwrap();
        }
        memory
            .write(GuestAddress(0x1010), &[0xb8, 1, 2, 3, 4])
            .unwrap();
        memory.write(GuestAddress(u32::MAX), &[0x0f]).unwrap();
        memory.write(GuestAddress(0), &[0x77]).unwrap();
        let mut segments = SegmentContext::default();
        segments.state.bases[1] = 0xffff_f000;
        let cs = CodeFetchContext::from_segments(&segments).unwrap();
        let BoundDecode::Instruction(decoded) =
            decode_bound(&memory, GuestAddress(0x2010), cs, DecodePolicy::Strict).unwrap()
        else {
            panic!("instruction expected")
        };
        assert_eq!(decoded.next_pc(), GuestAddress(0x2015));
        assert!(memory.is_code_current(decoded.code_snapshot()));
        for (policy, address, byte) in [
            (DecodePolicy::Strict, u32::MAX, 0x0f),
            (DecodePolicy::Execution, 0, 0x77),
        ] {
            let BoundDecode::Instruction(decoded) =
                decode_bound(&memory, GuestAddress(0xfff), cs, policy).unwrap()
            else {
                panic!("instruction expected")
            };
            assert_eq!(
                (decoded.pc(), decoded.next_pc(), decoded.length()),
                (GuestAddress(0xfff), GuestAddress(0x1001), 2)
            );
            assert!(memory.is_code_current(decoded.code_snapshot()));
            memory.write(GuestAddress(address), &[byte]).unwrap();
            assert!(!memory.is_code_current(decoded.code_snapshot()));
        }
    }

    #[test]
    fn based_page_fault_reports_linear_address_and_logical_faulting_pc() {
        let memory = AddressSpace::new(1).unwrap();
        let mut segments = SegmentContext::default();
        segments.state.bases[1] = 0x4000;
        let cs = CodeFetchContext::from_segments(&segments).unwrap();
        assert!(matches!(
            decode_bound(&memory, GuestAddress(0x1000), cs, DecodePolicy::Strict),
            Err(DecodeError::Paging {
                pc: GuestAddress(0x1000),
                error: CpuPagingError32::PageFault(fault),
                attempted_length: 1
            }) if fault.linear() == GuestAddress(0x5000) && fault.error_code() == 20
        ));
    }
    #[test]
    fn based_linear_wrap_reports_missing_low_page_with_logical_instruction_pc() {
        let mut memory = AddressSpace::new(1).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0xfffff000), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        memory.write(GuestAddress(u32::MAX), &[0x0f]).unwrap();
        let mut segments = SegmentContext::default();
        segments.state.bases[1] = 0xfffff000;
        let cs = CodeFetchContext::from_segments(&segments).unwrap();
        assert!(matches!(
            decode_bound(&memory, GuestAddress(0xfff), cs, DecodePolicy::Strict),
            Err(DecodeError::Paging {
                pc: GuestAddress(0xfff),
                attempted_length: 2,
                error: CpuPagingError32::PageFault(fault),
            }) if fault.linear() == GuestAddress(0) && fault.error_code() == 20
        ));
        let BoundDecode::CodeFault(fault) =
            decode_bound(&memory, GuestAddress(0xfff), cs, DecodePolicy::Execution).unwrap()
        else {
            panic!("bound execution retains the precise wrapped-fetch stop")
        };
        assert_eq!(fault.pc, GuestAddress(0xfff));
        assert_eq!(
            (fault.attempted_length, fault.consumed_prefix_length),
            (2, 1)
        );
        assert!(fault.remains_denied(&memory, cs));
        assert!(memory.is_code_current(fault.prefix.as_ref().unwrap()));
    }
}

#[cfg(test)]
mod wrap_mode_tests {
    use super::*;
    use crate::{
        cpu::x86::{SegmentCache, SegmentContext},
        memory::{PageRange, Permissions},
    };

    #[test]
    fn translated_wrap_keeps_code_default_size_independent_of_stack_pointer_size() {
        let mut memory = AddressSpace::new(2).unwrap();
        for page in [0, 0xfffff000] {
            memory
                .map_zeroed(
                    PageRange::new(GuestAddress(page), 1).unwrap(),
                    Permissions::ALL,
                )
                .unwrap();
        }
        memory.write(GuestAddress(u32::MAX), &[0xb8]).unwrap();
        memory
            .write(GuestAddress(0), &[0xcd, 0xab, 0x34, 0x12])
            .unwrap();
        for default32 in [false, true] {
            for stack32 in [false, true] {
                let mut segments = SegmentContext::default();
                segments.state.bases[1] = 0xffffe000;
                for (index, wide) in [(1, default32), (2, stack32)] {
                    segments.caches[index] = SegmentCache::from_bits(
                        (segments.caches[index].bits() & !(1 << 25)) | (u32::from(wide) << 25),
                    )
                    .unwrap();
                }
                let cs = CodeFetchContext::from_segments(&segments).unwrap();
                let BoundDecode::Instruction(instruction) =
                    decode_bound(&memory, GuestAddress(0x1fff), cs, DecodePolicy::Strict).unwrap()
                else {
                    panic!("instruction expected")
                };
                assert_eq!(
                    (instruction.length(), instruction.next_pc()),
                    if default32 {
                        (5, GuestAddress(0x2004))
                    } else {
                        (3, GuestAddress(0x2002))
                    }
                );
                assert_eq!(
                    matches!(instruction.operation(), Operation::Move { .. }),
                    default32
                );
                assert_eq!(
                    matches!(instruction.operation(), Operation::MoveWord { .. }),
                    !default32
                );
                assert_eq!(instruction.code_snapshot().versions.len(), 2);
                assert!(memory.is_code_current(instruction.code_snapshot()));
            }
        }
    }

    #[test]
    fn logical_four_gib_crossing_stays_denied_before_unmapped_second_linear_byte() {
        let mut memory = AddressSpace::new(1).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        memory.write(GuestAddress(0xfff), &[0x0f]).unwrap();
        for default32 in [false, true] {
            let mut segments = SegmentContext::default();
            segments.state.bases[1] = 0x1000;
            segments.caches[1] = SegmentCache::from_bits(
                (segments.caches[1].bits() & !(1 << 25)) | (u32::from(default32) << 25),
            )
            .unwrap();
            let cs = CodeFetchContext::from_segments(&segments).unwrap();
            assert!(matches!(
                decode_bound(&memory, GuestAddress(u32::MAX), cs, DecodePolicy::Strict),
                Err(DecodeError::CodeSegment {
                    pc: GuestAddress(u32::MAX),
                    attempted_length: 2,
                })
            ));
            let BoundDecode::CodeFault(fault) =
                decode_bound(&memory, GuestAddress(u32::MAX), cs, DecodePolicy::Execution).unwrap()
            else {
                panic!("logical code fault expected")
            };
            assert_eq!(
                (
                    fault.pc,
                    fault.attempted_length,
                    fault.consumed_prefix_length
                ),
                (GuestAddress(u32::MAX), 2, 1)
            );
            assert!(memory.is_code_current(fault.prefix.as_ref().unwrap()));
        }
    }
}
