use super::{BoundDecode, CapturedCodeFault, CodeFetchContext};
use iced_x86::{Decoder, DecoderError, DecoderOptions};

use crate::{
    cpu::{
        UnsupportedFeature,
        x86::{environment::USER_MODE_ENVIRONMENT, ir::Operation},
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
    InvalidEncoding,
    Unsupported(UnsupportedFeature),
    Infrastructure(MemoryError),
}

#[derive(Debug)]
pub struct DecodedInstruction {
    pc: GuestAddress,
    length: u8,
    next_pc: GuestAddress,
    operation: Operation,
    code_snapshot: CodeSnapshot,
    segment_accesses: u8,
}

impl DecodedInstruction {
    pub(crate) fn uses_segments(&self) -> bool {
        self.segment_accesses != 0
    }
    pub(crate) fn segment_accesses(&self) -> u8 {
        self.segment_accesses
    }
    pub fn pc(&self) -> GuestAddress {
        self.pc
    }

    pub fn length(&self) -> u8 {
        self.length
    }

    pub fn next_pc(&self) -> GuestAddress {
        self.next_pc
    }

    pub fn operation(&self) -> &Operation {
        &self.operation
    }

    pub fn code_snapshot(&self) -> &CodeSnapshot {
        &self.code_snapshot
    }
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
    unbound(decode(memory, pc, DecodePolicy::Strict, None)?)
}

pub(crate) fn decode_for_execution(
    memory: &AddressSpace,
    pc: GuestAddress,
) -> Result<DecodedInstruction, DecodeError> {
    unbound(decode(memory, pc, DecodePolicy::Execution, None)?)
}

fn unbound(decoded: BoundDecode) -> Result<DecodedInstruction, DecodeError> {
    match decoded {
        BoundDecode::Instruction(instruction) => Ok(instruction),
        BoundDecode::CodeFault(_) => unreachable!("unbound decoding has no CS authority"),
    }
}

pub(crate) fn decode_bound(
    memory: &AddressSpace,
    pc: GuestAddress,
    cs: CodeFetchContext,
    policy: DecodePolicy,
) -> Result<BoundDecode, DecodeError> {
    decode(memory, pc, policy, Some(cs))
}

fn decode(
    memory: &AddressSpace,
    pc: GuestAddress,
    policy: DecodePolicy,
    cs: Option<CodeFetchContext>,
) -> Result<BoundDecode, DecodeError> {
    let linear = cs.map_or(pc, |cs| cs.linear_address(pc));
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
                    memory
                        .snapshot_code(linear, consumed)
                        .map_err(|error| memory_error(error, pc, consumed))?,
                )
            };
            return Ok(BoundDecode::CodeFault(CapturedCodeFault {
                pc,
                attempted_length: length as u8,
                consumed_prefix_length: consumed as u8,
                prefix,
            }));
        }
        if cs.is_some_and(|cs| cs.linear_span(pc, length).is_none()) {
            return Err(DecodeError::Unsupported(UnsupportedFeature::Segment));
        }
        memory
            .fetch(linear, &mut bytes[..length])
            .map_err(|error| memory_error(error, pc, length))?;
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
                    return finish(
                        memory,
                        pc,
                        linear,
                        length,
                        GuestAddress(pc.0.wrapping_add(length as u32)),
                        Operation::Control(operation),
                        0,
                    )
                    .map(BoundDecode::Instruction);
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
                    match super::lower::lower(&instruction, &bytes[..consumed]) {
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
                return finish(
                    memory,
                    pc,
                    linear,
                    consumed,
                    GuestAddress(instruction.next_ip32()),
                    operation,
                    segments,
                )
                .map(BoundDecode::Instruction);
            }
            _ => return Err(DecodeError::InvalidEncoding),
        }
    }
    Err(DecodeError::InvalidEncoding)
}

fn finish(
    memory: &AddressSpace,
    pc: GuestAddress,
    linear: GuestAddress,
    length: usize,
    next_pc: GuestAddress,
    operation: Operation,
    segment_accesses: u8,
) -> Result<DecodedInstruction, DecodeError> {
    let code_snapshot = memory
        .snapshot_code(linear, length)
        .map_err(|error| memory_error(error, pc, length))?;
    Ok(DecodedInstruction {
        pc,
        length: length as u8,
        next_pc,
        operation,
        code_snapshot,
        segment_accesses,
    })
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

fn segment_accesses(instruction: &iced_x86::Instruction) -> u8 {
    use iced_x86::{InstructionInfoFactory, Mnemonic, Register};
    if matches!(
        instruction.mnemonic(),
        Mnemonic::Lea
            | Mnemonic::Nop
            | Mnemonic::Prefetchnta
            | Mnemonic::Prefetcht0
            | Mnemonic::Prefetcht1
            | Mnemonic::Prefetcht2
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
            &[0xcd, 0x80][..],
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
            assert!(
                matches!(strict, DecodeError::MemoryFault { length, .. } if length == expected_length),
                "{strict:?}"
            );
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
    fn based_fetch_accepts_wrapped_base_addition_but_refuses_a_split_linear_instruction() {
        let mut memory = AddressSpace::new(2).unwrap();
        for page in [0x1000, 0xffff_f000] {
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
        for policy in [DecodePolicy::Strict, DecodePolicy::Execution] {
            assert!(matches!(
                decode_bound(&memory, GuestAddress(0xfff), cs, policy),
                Err(DecodeError::Unsupported(UnsupportedFeature::Segment))
            ));
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
            Err(DecodeError::MemoryFault {
                pc: GuestAddress(0x1000),
                fault: MemoryFault {
                    address: GuestAddress(0x5000),
                    ..
                },
                length: 1
            })
        ));
    }
}
