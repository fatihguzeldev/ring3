use super::{IndependentSession, memory_failure, paging_failure};
use crate::{
    cpu::{
        runtime::{CpuState, ServiceSite, Status, numeric::NumericError},
        x86::{
            ControlOperation, PagingAccess32, Register32, SegmentAccess, SegmentContext,
            SegmentRegister, StackPointerSize, State32, USER_MODE_ENVIRONMENT, X87Operation,
            capabilities::CPU_ID_LEAVES,
            decode::{
                BoundDecode, DecodeAuthority, DecodeError, DecodePolicy, DecodedOperation,
                decode_for_compilation,
            },
            ir::{AddressSize, EffectiveAddress, Operation},
            read_cpu_paging_access32,
        },
    },
    memory::{AddressSpace, GuestAddress, MemoryError},
    process::{EngineInstance, HostError},
};

mod stack;

enum Effect {
    Complete,
    Fault { vector: u32, detail: u32 },
    Trap(u32),
    SystemEntry(u32),
    Unsupported,
}

fn preserves_memory(operation: Operation) -> bool {
    let operation = match operation {
        Operation::X87 { operation, .. } => operation,
        Operation::InitializeX87 => X87Operation::Initialize,
        Operation::ClearX87Exceptions => X87Operation::ClearExceptions,
        Operation::X87StatusToAx => X87Operation::StatusToAx,
        _ => return false,
    };
    operation.write_size() == 0
        && matches!(
            operation,
            X87Operation::Initialize
                | X87Operation::ClearExceptions
                | X87Operation::Wait
                | X87Operation::Load(_)
                | X87Operation::LoadRegister { .. }
                | X87Operation::StoreRegister { .. }
                | X87Operation::StoreRegisterNoUnderflow { .. }
                | X87Operation::Exchange { .. }
                | X87Operation::Binary { .. }
                | X87Operation::BinaryMemory { .. }
                | X87Operation::Compare { .. }
                | X87Operation::CompareMemory { .. }
                | X87Operation::ConditionalMove { .. }
                | X87Operation::Test
                | X87Operation::Examine
                | X87Operation::ChangeSign
                | X87Operation::Absolute
                | X87Operation::SquareRoot
                | X87Operation::RoundToInteger
                | X87Operation::Scale
                | X87Operation::Extract
                | X87Operation::Remainder { .. }
                | X87Operation::LoadConstant { .. }
                | X87Operation::Free { .. }
                | X87Operation::AdjustTop { .. }
                | X87Operation::LoadControl
                | X87Operation::StatusToAx
                | X87Operation::LoadEnvironment { .. }
                | X87Operation::Restore { .. }
                | X87Operation::NoOperation
                | X87Operation::Exp2MinusOne
                | X87Operation::Log2Multiply { .. }
                | X87Operation::PartialAtan
                | X87Operation::Trigonometric { .. }
        )
}

fn decode_instruction(
    memory: &AddressSpace,
    pc: GuestAddress,
) -> Result<DecodedOperation, DecodeError> {
    #[cfg(feature = "observability")]
    let _scope = ring3_observability::current().scope(
        ring3_observability::Level::Detailed,
        ring3_observability::categories::ENGINE_CPU_DECODE,
        "instruction",
        &[
            ring3_observability::Field::unsigned("pc", u64::from(pc.0)),
            ring3_observability::Field::unsigned("cache_hit", 0),
        ],
    );
    // this is decode_one's strict, unbound decode without its immediately dropped
    // currency snapshot; the immutable fetch already proves the consumed span.
    match decode_for_compilation(
        memory,
        pc,
        DecodePolicy::Strict,
        None,
        DecodeAuthority::default(),
    )? {
        BoundDecode::Instruction(instruction) => Ok(instruction),
        // strict unbound decoding cannot capture a deferred code fault.
        BoundDecode::CodeFault(_) => Err(DecodeError::Infrastructure(MemoryError::InvalidRange)),
    }
}

pub(super) fn execute(
    instance: &mut EngineInstance,
    session: &mut IndependentSession,
) -> Result<(), HostError> {
    if session.state.status != Status::Instruction as u32 || session.state.remaining == 0 {
        session.memory_cache.invalidate();
        return Err(HostError::InvalidRequest);
    }
    let Some(memory) = instance.memory.as_mut() else {
        session.memory_cache.invalidate();
        return Err(HostError::Closed);
    };
    session.publish_dirty_preserving_memory(memory)?;
    let pc = session.state.eip;
    let service = session.cache.service(memory, pc).map_err(|error| {
        session.memory_cache.invalidate();
        error
    })?;
    let instruction = match service {
        Some(site) => {
            #[cfg(feature = "observability")]
            let _scope = ring3_observability::current().scope(
                ring3_observability::Level::Detailed,
                ring3_observability::categories::ENGINE_CPU_DECODE,
                "instruction",
                &[
                    ring3_observability::Field::unsigned("pc", u64::from(pc)),
                    ring3_observability::Field::unsigned("cache_hit", 1),
                ],
            );
            site
        }
        None => match decode_instruction(memory, GuestAddress(pc)) {
            Ok(decoded) => ServiceSite {
                pc: decoded.pc().0,
                next_pc: decoded.next_pc().0,
                operation: *decoded.operation(),
            },
            Err(error) => {
                session.memory_cache.invalidate();
                decode_failure(&mut session.state, error)?;
                session.refresh(memory);
                return Ok(());
            }
        },
    };
    let operation = instruction.operation;
    let preserve_memory = preserves_memory(operation);
    if !preserve_memory {
        session.memory_cache.invalidate();
    }
    let next = instruction.next_pc;
    let before = memory.code_validation_version();
    let mut scalar = session.state.state();
    let result =
        match session
            .numeric
            .execute(memory, &mut scalar, &session.segments, &operation, next)
        {
            Ok(_) => Ok(Effect::Complete),
            Err(NumericError::Unsupported) if preserve_memory => {
                session.memory_cache.invalidate();
                Ok(Effect::Unsupported)
            }
            Err(NumericError::Unsupported) => {
                if processor(instance.cpu_clock.timestamp(), &mut scalar, operation)? {
                    Ok(Effect::Complete)
                } else {
                    match stack::execute(memory, &session.segments, &mut scalar, operation) {
                        Ok(true) => Ok(Effect::Complete),
                        Ok(false) => control(memory, &session.segments, &mut scalar, operation),
                        Err(error) => Err(error),
                    }
                }
            }
            Err(error) => Err(error),
        };
    match result {
        Ok(Effect::Complete) => {
            scalar.eip = next;
            session.state.set_state(scalar);
            retire(
                &mut session.state,
                if memory.code_validation_version() != before {
                    Status::CodeInvalidated
                } else {
                    Status::Continue
                },
            );
        }
        Ok(Effect::Trap(vector)) => {
            scalar.eip = next;
            session.state.set_state(scalar);
            retire(&mut session.state, Status::CpuFault);
            session.state.vector = vector;
            session.state.detail = u32::from(vector == 1);
            session.state.auxiliary = instruction.pc;
        }
        Ok(Effect::SystemEntry(vector)) => {
            scalar.eip = next;
            session.state.set_state(scalar);
            retire(&mut session.state, Status::SystemEntry);
            session.state.vector = vector;
            session.state.detail = instruction.pc;
            session.state.auxiliary = 0;
        }
        Ok(Effect::Fault { vector, detail }) | Err(NumericError::Cpu { vector, detail }) => {
            session.state.status = Status::CpuFault as u32;
            session.state.vector = vector;
            session.state.detail = detail;
            session.state.auxiliary = 0;
        }
        Ok(Effect::Unsupported) | Err(NumericError::Unsupported) => {
            session.state.status = Status::Unsupported as u32
        }
        Err(NumericError::Memory { error, width }) => {
            paging_failure(&mut session.state, error, width)
        }
        Err(NumericError::Infrastructure) => {
            session.memory_cache.invalidate();
            return Err(HostError::Infrastructure);
        }
    }
    if preserve_memory {
        session.refresh_preserving_memory(memory);
    } else {
        session.refresh(memory);
    }
    Ok(())
}

fn retire(state: &mut CpuState, status: Status) {
    state.remaining -= 1;
    state.retired = state.retired.wrapping_add(1);
    state.status = status as u32;
}

pub(super) fn decode_failure(state: &mut CpuState, error: DecodeError) -> Result<(), HostError> {
    match error {
        DecodeError::MemoryFault { fault, length, .. } => {
            memory_failure(state, MemoryError::Fault(fault), length)
        }
        DecodeError::Paging {
            error,
            attempted_length,
            ..
        } => paging_failure(state, error, usize::from(attempted_length)),
        DecodeError::CodeSegment { .. } => {
            state.status = Status::CpuFault as u32;
            state.vector = 13;
            state.detail = 0;
            state.auxiliary = 0;
        }
        DecodeError::Infrastructure(_) => return Err(HostError::Infrastructure),
        _ => state.status = Status::Unsupported as u32,
    }
    Ok(())
}

fn processor(
    timestamp: Result<u64, HostError>,
    scalar: &mut State32,
    operation: Operation,
) -> Result<bool, HostError> {
    match operation {
        Operation::CpuId => {
            let leaf = scalar.registers[Register32::Eax.index()];
            let subleaf = scalar.registers[Register32::Ecx.index()];
            let values = CPU_ID_LEAVES
                .iter()
                .find(|candidate| {
                    candidate.function == leaf
                        && candidate.subfunction.is_none_or(|sub| sub == subleaf)
                })
                .map_or([0; 4], |leaf| leaf.registers);
            for (register, value) in [
                Register32::Eax,
                Register32::Ebx,
                Register32::Ecx,
                Register32::Edx,
            ]
            .into_iter()
            .zip(values)
            {
                scalar.registers[register.index()] = value;
            }
        }
        Operation::ReadProcessorId { destination } => {
            scalar.registers[destination.index()] = USER_MODE_ENVIRONMENT.tsc_aux()
        }
        Operation::VirtualTimestamp | Operation::VirtualTimestampAux => {
            let timestamp = timestamp?;
            scalar.registers[Register32::Eax.index()] = timestamp as u32;
            scalar.registers[Register32::Edx.index()] = (timestamp >> 32) as u32;
            if operation == Operation::VirtualTimestampAux {
                scalar.registers[Register32::Ecx.index()] = USER_MODE_ENVIRONMENT.tsc_aux();
            }
        }
        _ => return Ok(false),
    }
    Ok(true)
}

fn read(
    memory: &AddressSpace,
    segments: &SegmentContext,
    segment: SegmentRegister,
    offset: u32,
    output: &mut [u8],
) -> Result<(), NumericError> {
    segments.caches[segment.index()]
        .check_span(offset, output.len() as u32, SegmentAccess::Read)
        .map_err(|_| NumericError::Cpu {
            vector: if segment == SegmentRegister::Ss {
                12
            } else {
                13
            },
            detail: 0,
        })?;
    let linear = segments.state.bases[segment.index()].wrapping_add(offset);
    read_cpu_paging_access32(memory, GuestAddress(linear), output, PagingAccess32::Read).map_err(
        |error| NumericError::Memory {
            error,
            width: output.len(),
        },
    )
}

fn effective_offset(address: EffectiveAddress, state: &State32) -> u32 {
    let get = |register: Register32| state.registers[register.index()];
    let value = address
        .displacement
        .wrapping_add(address.base.map_or(0, get))
        .wrapping_add(
            address
                .index
                .map_or(0, |index| get(index).wrapping_mul(u32::from(address.scale))),
        );
    if address.address_size == AddressSize::Bits16 {
        value & 0xffff
    } else {
        value
    }
}

fn control(
    memory: &mut AddressSpace,
    segments: &SegmentContext,
    scalar: &mut State32,
    operation: Operation,
) -> Result<Effect, NumericError> {
    Ok(match operation {
        Operation::SoftwareInterrupt { vector } => Effect::SystemEntry(u32::from(vector)),
        Operation::Control(ControlOperation::Sysenter) => Effect::SystemEntry(0x100),
        Operation::Control(ControlOperation::InvalidOpcode) => Effect::Fault {
            vector: 6,
            detail: 0,
        },
        Operation::Control(ControlOperation::GeneralProtectionZero) => Effect::Fault {
            vector: 13,
            detail: 0,
        },
        Operation::Control(ControlOperation::Breakpoint) => Effect::Trap(3),
        Operation::Control(ControlOperation::DebugTrap) => Effect::Trap(1),
        Operation::Control(ControlOperation::Overflow) if scalar.eflags & 0x800 != 0 => {
            Effect::Trap(4)
        }
        Operation::Control(ControlOperation::Overflow) => Effect::Complete,
        Operation::Control(ControlOperation::DeniedPortString { repeat_count }) => {
            let denied = repeat_count.is_none_or(|size| {
                if size == AddressSize::Bits16 {
                    scalar.registers[1] & 0xffff != 0
                } else {
                    scalar.registers[1] != 0
                }
            });
            if denied {
                Effect::Fault {
                    vector: 13,
                    detail: 0,
                }
            } else {
                Effect::Complete
            }
        }
        Operation::Control(ControlOperation::PopFlags { word }) => {
            let width = if word { 2 } else { 4 };
            let pointer =
                StackPointerSize::new(segments.caches[SegmentRegister::Ss.index()].default32());
            let mut bytes = [0; 4];
            read(
                memory,
                segments,
                SegmentRegister::Ss,
                pointer.offset(scalar.registers[4]),
                &mut bytes[..width],
            )?;
            let flags =
                crate::cpu::x86::pop_flags(scalar.eflags, u32::from_le_bytes(bytes), word) | 2;
            if flags & 0x50100 != 0 {
                return Ok(Effect::Unsupported);
            }
            scalar.eflags = flags;
            scalar.registers[4] = pointer.advance(scalar.registers[4], width as i32);
            Effect::Complete
        }
        Operation::Control(ControlOperation::Bound {
            index,
            address,
            word,
        }) => {
            let mut bytes = [0; 8];
            read(
                memory,
                segments,
                address.resolved_segment(),
                effective_offset(address, scalar),
                &mut bytes[..if word { 4 } else { 8 }],
            )?;
            let (value, lower, upper) = if word {
                (
                    i32::from(scalar.registers[index.index()] as i16),
                    i32::from(i16::from_le_bytes(bytes[..2].try_into().unwrap())),
                    i32::from(i16::from_le_bytes(bytes[2..4].try_into().unwrap())),
                )
            } else {
                (
                    scalar.registers[index.index()] as i32,
                    i32::from_le_bytes(bytes[..4].try_into().unwrap()),
                    i32::from_le_bytes(bytes[4..].try_into().unwrap()),
                )
            };
            if value < lower || value > upper {
                Effect::Fault {
                    vector: 5,
                    detail: 0,
                }
            } else {
                Effect::Complete
            }
        }
        _ => Effect::Unsupported,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{PageRange, Permissions};

    #[test]
    fn cpuid_and_timestamp_use_existing_profile_and_fixed_observation() {
        let mut state = State32::default();
        assert!(processor(Err(HostError::Infrastructure), &mut state, Operation::CpuId).unwrap());
        assert_eq!(
            (
                state.registers[0],
                state.registers[3],
                state.registers[1],
                state.registers[2]
            ),
            (13, 0x676e6952, 0x20656e69, 0x676e4533)
        );
        assert!(
            processor(
                Ok(0x1234567887654321),
                &mut state,
                Operation::VirtualTimestampAux
            )
            .unwrap()
        );
        assert_eq!(
            (state.registers[0], state.registers[2], state.registers[1]),
            (0x87654321, 0x12345678, 0)
        );
        let before = state;
        assert!(
            processor(
                Ok(0x1234567887654321),
                &mut state,
                Operation::VirtualTimestampAux
            )
            .unwrap()
        );
        assert_eq!(state, before);
    }

    #[test]
    fn popflags_late_read_fault_leaves_general_state_unchanged() {
        let mut memory = AddressSpace::new(1).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                Permissions::READ_WRITE,
            )
            .unwrap();
        let mut scalar = State32 {
            registers: [0, 0, 0, 0, 0x1ffe, 0, 0, 0],
            ..Default::default()
        };
        let before = scalar;
        assert!(matches!(
            control(
                &mut memory,
                &SegmentContext::default(),
                &mut scalar,
                Operation::Control(ControlOperation::PopFlags { word: false })
            ),
            Err(NumericError::Memory { width: 4, .. })
        ));
        assert_eq!(scalar, before);
    }

    #[test]
    fn denied_zero_count_port_string_does_not_access_memory() {
        let mut memory = AddressSpace::new(1).unwrap();
        let mut scalar = State32 {
            registers: [0, 0xffff0000, 0, 0, 0, 0, 0, 0],
            ..Default::default()
        };
        assert!(matches!(
            control(
                &mut memory,
                &SegmentContext::default(),
                &mut scalar,
                Operation::Control(ControlOperation::DeniedPortString {
                    repeat_count: Some(AddressSize::Bits16)
                })
            )
            .unwrap(),
            Effect::Complete
        ));
        assert!(matches!(
            control(
                &mut memory,
                &SegmentContext::default(),
                &mut scalar,
                Operation::Control(ControlOperation::DeniedPortString {
                    repeat_count: Some(AddressSize::Bits32)
                })
            )
            .unwrap(),
            Effect::Fault {
                vector: 13,
                detail: 0
            }
        ));
    }

    #[test]
    fn popflags_rejects_unsupported_trap_flag_before_scalar_effects() {
        let mut memory = AddressSpace::new(1).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                Permissions::READ_WRITE,
            )
            .unwrap();
        memory
            .write(GuestAddress(0x1000), &0x102_u32.to_le_bytes())
            .unwrap();
        let mut scalar = State32 {
            registers: [0, 0, 0, 0, 0x1000, 0, 0, 0],
            ..Default::default()
        };
        let before = scalar;
        assert!(matches!(
            control(
                &mut memory,
                &SegmentContext::default(),
                &mut scalar,
                Operation::Control(ControlOperation::PopFlags { word: false })
            )
            .unwrap(),
            Effect::Unsupported
        ));
        assert_eq!(scalar, before);
    }
}
