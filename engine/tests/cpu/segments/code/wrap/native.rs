use ring3_engine::{
    abi::{
        arena::{SEGMENT_BASES_RECORD_OFFSET, SEGMENTS_OFFSET, TRANSFER_OFFSET},
        x86::{SEGMENT_BASES_RECORD_SIZE, SEGMENTS_SIZE, encode_current_segment_context},
    },
    cpu::{
        dbt::{CompileError, InstructionError, RegistryError},
        x86::{SegmentCache, SegmentRegister, decode::DecodeError},
    },
    memory::{Access, FaultReason, GuestAddress, MemoryFault},
    process::{EngineInstance, HostError},
};
const KEY: u64 = 0x435357524150;
fn upload(e: &mut EngineInstance, address: u32, bytes: &[u8]) {
    e.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()].copy_from_slice(bytes);
    e.upload(address, bytes.len() as u32).unwrap();
}
fn context(e: &mut EngineInstance, limit: Option<u32>) {
    let mut c = e.segment_context().unwrap();
    c.state.bases[1] = 0xffffe000;
    if let Some(limit) = limit {
        c.caches[1] = SegmentCache::from_bits(0xf2b00000 | limit).unwrap();
    }
    let mut sg = [0; SEGMENTS_SIZE];
    let mut sb = [0; SEGMENT_BASES_RECORD_SIZE];
    encode_current_segment_context(&c, &mut sg, &mut sb).unwrap();
    e.arena_mut().unwrap()[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE].copy_from_slice(&sg);
    e.arena_mut().unwrap()
        [SEGMENT_BASES_RECORD_OFFSET..SEGMENT_BASES_RECORD_OFFSET + SEGMENT_BASES_RECORD_SIZE]
        .copy_from_slice(&sb);
}
fn descriptors(e: &mut EngineInstance, entries: bool, pc: u32, length: u32, gate: bool) {
    let mut words = if entries { vec![pc] } else { vec![pc, length] };
    if gate {
        words.extend([pc, 77]);
    }
    for (i, v) in words.into_iter().enumerate() {
        e.arena_mut().unwrap()[TRANSFER_OFFSET + i * 4..TRANSFER_OFFSET + i * 4 + 4]
            .copy_from_slice(&v.to_le_bytes());
    }
}
fn compile(
    e: &mut EngineInstance,
    resident: bool,
    entries: bool,
    gate: bool,
) -> Result<u64, HostError> {
    match (resident, entries) {
        (false, false) => e.compile_with_gates(1, u32::from(gate)).map(u64::from),
        (false, true) => e.compile_entries(1, u32::from(gate)).map(u64::from),
        (true, false) => e
            .compile_resident_with_gates(1, u32::from(gate))
            .map(|id| id.get()),
        (true, true) => e
            .compile_resident_entries(1, u32::from(gate))
            .map(|id| id.get()),
    }
}
fn guard(e: &EngineInstance, resident: bool, token: u64) -> Result<(), HostError> {
    if resident {
        e.guard_resident(KEY, token)
    } else {
        e.guard(KEY, token as u32)
    }
}
fn stale(resident: bool) -> HostError {
    if resident {
        HostError::Resident(RegistryError::CodeInvalidated)
    } else {
        HostError::CodeInvalidated
    }
}
fn cause(error: HostError) -> CompileError {
    match error {
        HostError::Compile(e) | HostError::Resident(RegistryError::Compile(e)) => e,
        other => panic!("typed compile error expected: {other:?}"),
    }
}
fn mapped(low: Option<u32>) -> EngineInstance {
    let mut e = EngineInstance::new(3, KEY).unwrap();
    e.map(0xfffff000, 1, 7).unwrap();
    e.map(0x1000, 1, 7).unwrap();
    if let Some(bits) = low {
        e.map(0, 1, bits).unwrap();
    }
    context(&mut e, None);
    e
}

#[test]
fn split_linear_instruction_compiles_and_snapshots_both_physical_halves_in_all_profiles() {
    for resident in [false, true] {
        for entries in [false, true] {
            for address in [0xfffffffd, 0] {
                let mut e = mapped(Some(7));
                upload(&mut e, 0xfffffffd, &[0xb8, 0x78, 0x56]);
                upload(&mut e, 0, &[0x34, 0x12, 0xeb, 0xfe]);
                descriptors(&mut e, entries, 0x1ffd, 7, false);
                let token = compile(&mut e, resident, entries, false).unwrap();
                assert_eq!(guard(&e, resident, token), Ok(()));
                upload(&mut e, 0x1ffd, &[0xb8]);
                assert_eq!(guard(&e, resident, token), Ok(()));
                upload(&mut e, address, &[if address == 0 { 0x34 } else { 0xb8 }]);
                crate::support_fetch::assert_host_contract(
                    (guard(&e, resident, token)).expect_err("compilation must refuse this fixture"),
                    stale(resident),
                );
            }
        }
    }
}

#[test]
fn wrapped_gate_compiles_and_keeps_each_source_page_currentness() {
    for resident in [false, true] {
        for entries in [false, true] {
            for address in [u32::MAX, 0] {
                let mut e = mapped(Some(7));
                upload(&mut e, u32::MAX, &[0x0f]);
                upload(&mut e, 0, &[0x0b]);
                descriptors(&mut e, entries, 0x1fff, 2, true);
                let token = compile(&mut e, resident, entries, true).unwrap();
                assert_eq!(guard(&e, resident, token), Ok(()));
                upload(&mut e, address, &[if address == 0 { 0x0b } else { 0x0f }]);
                crate::support_fetch::assert_host_contract(
                    (guard(&e, resident, token)).expect_err("compilation must refuse this fixture"),
                    stale(resident),
                );
            }
        }
    }
}

#[test]
fn wrapped_tail_missing_or_nonexecute_is_typed_low_linear_fault_with_atomic_admission() {
    for resident in [false, true] {
        for entries in [false, true] {
            for low in [None, Some(3)] {
                let mut e = mapped(low);
                upload(&mut e, u32::MAX, &[0x66]);
                if low.is_some() {
                    upload(&mut e, 0, &[0x90, 0xeb, 0xfe]);
                }
                descriptors(&mut e, entries, 0x1fff, 4, false);
                let before = e.arena().to_vec();
                let generation = e.generation();
                let error = compile(&mut e, resident, entries, false).unwrap_err();
                crate::support_fetch::assert_compile_contract(
                    cause(error),
                    CompileError::Instruction {
                        pc: GuestAddress(0x1fff),
                        cause: InstructionError::Decode(DecodeError::MemoryFault {
                            pc: GuestAddress(0x1fff),
                            length: 2,
                            fault: MemoryFault {
                                address: GuestAddress(0),
                                access: Access::Execute,
                                reason: if low.is_some() {
                                    FaultReason::Permission
                                } else {
                                    FaultReason::Unmapped
                                },
                            },
                        }),
                    },
                );
                assert_eq!(e.arena(), before);
                assert_eq!(e.generation(), generation);
                let mut high = [0; 1];
                e.memory()
                    .unwrap()
                    .read(GuestAddress(u32::MAX), &mut high)
                    .unwrap();
                assert_eq!(high, [0x66]);
                if low.is_some() {
                    e.protect(0, 1, 7).unwrap();
                    descriptors(&mut e, entries, 0x1fff, 4, false);
                    assert!(compile(&mut e, resident, entries, false).is_ok());
                }
            }
        }
    }
}

#[test]
fn logical_cs_limit_precedes_unmapped_translated_tail() {
    for resident in [false, true] {
        for entries in [false, true] {
            let mut e = mapped(None);
            upload(&mut e, u32::MAX, &[0x66]);
            context(&mut e, Some(0x1fff));
            descriptors(&mut e, entries, 0x1fff, 2, false);
            let before = e.arena().to_vec();
            assert_eq!(
                cause(compile(&mut e, resident, entries, false).unwrap_err()),
                CompileError::Instruction {
                    pc: GuestAddress(0x1fff),
                    cause: InstructionError::Decode(DecodeError::CodeSegment {
                        pc: GuestAddress(0x1fff),
                        attempted_length: 2
                    })
                }
            );
            assert_eq!(e.arena(), before);
            assert_eq!(e.generation(), 0);
            assert_eq!(
                e.segment_context().unwrap().state.bases[SegmentRegister::Cs.index()],
                0xffffe000
            );
        }
    }
}
