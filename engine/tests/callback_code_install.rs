use ring3_engine::abi::x86::{decode_exit, decode_state, encode_exit_v3, encode_state};
use ring3_engine::cpu::dbt::{CompileError, InstructionError};
use ring3_engine::cpu::x86::{State32, decode::DecodeError};
use ring3_engine::cpu::{ExecutionExit, ExitReason, UnsupportedFeature};
use ring3_engine::memory::{Access, FaultReason, GuestAddress, MemoryFault};
use ring3_engine::process::{CallError, EngineInstance, HostError};
use ring3_engine::windows::CallingConvention32;

const KEY: u64 = 0xfedc_ba98_1234_5678;
const OUTER: u32 = 0x1000;
const ENTRY: u32 = 0x1100;
const TAIL: u32 = 0x1105;
const RETURN: u32 = 0x1200;
const INNER: u32 = 0x1300;
const TARGET: u32 = 0x4000;
const NEXT: u32 = 0x5000;

fn upload(engine: &mut EngineInstance, address: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
    engine.upload(address, bytes.len() as u32).unwrap();
}

fn word(engine: &mut EngineInstance, address: u32, value: u32) {
    upload(engine, address, &value.to_le_bytes());
}

fn bytes(engine: &EngineInstance, address: u32, length: usize) -> Vec<u8> {
    let mut output = vec![0; length];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(address), &mut output)
        .unwrap();
    output
}

fn descriptors(engine: &mut EngineInstance, blocks: &[(u32, u32)], gates: &[(u32, u32)]) {
    let transfer = &mut engine.arena_mut().unwrap()[140..];
    transfer.fill(0xcc);
    for (index, (address, value)) in blocks.iter().chain(gates).enumerate() {
        transfer[index * 8..index * 8 + 4].copy_from_slice(&address.to_le_bytes());
        transfer[index * 8 + 4..index * 8 + 8].copy_from_slice(&value.to_le_bytes());
    }
}

fn replacement(engine: &mut EngineInstance, target: u32, length: u32) {
    descriptors(
        engine,
        &[(OUTER, 2), (RETURN, 2), (target, length), (TAIL, 4)],
        &[(OUTER, 17), (RETURN, 18)],
    );
}

fn original() -> State32 {
    State32 {
        registers: [
            0x89ab_cdef,
            0x1357_9bdf,
            0x2345_6789,
            0x3456_789a,
            0x8010,
            0x5678_9abc,
            0x6789_abcd,
            0x789a_bcde,
        ],
        eip: OUTER,
        eflags: 0xcd7,
    }
}

fn stop(engine: &mut EngineInstance, state: State32, reason: ExitReason, retired: u32) {
    // native injection supplies lifecycle stops; actual CALL/RET execution is independently proved.
    encode_state(&state, &mut engine.arena_mut().unwrap()[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit { retired, reason },
        &mut engine.arena_mut().unwrap()[56..96],
    )
    .unwrap();
}

fn active() -> EngineInstance {
    let mut engine = EngineInstance::new(6, KEY).unwrap();
    for address in [0x1000, TARGET, NEXT] {
        engine.map(address, 1, 7).unwrap();
    }
    engine.map(0x7000, 2, 7).unwrap();
    for address in [OUTER, RETURN, INNER] {
        upload(&mut engine, address, &[0x0f, 0x0b]);
    }
    upload(&mut engine, ENTRY, &[0xe8, 0xfb, 0x2e, 0, 0]);
    upload(&mut engine, TAIL, &[0x90, 0xc2, 8, 0]);
    upload(&mut engine, TARGET, &[0xb8, 0x78, 0x56, 0x34, 0x12, 0xc3]);
    upload(&mut engine, NEXT, &[0x90, 0xc3]);
    for (address, value) in [
        (0x8010, 0x2000),
        (0x8014, 0xfedc_ba98),
        (0x8018, 0x7654_3210),
    ] {
        word(&mut engine, address, value);
    }
    descriptors(
        &mut engine,
        &[(OUTER, 2), (ENTRY, 5), (TAIL, 4), (RETURN, 2), (INNER, 2)],
        &[(OUTER, 17), (RETURN, 18), (INNER, 19)],
    );
    assert_eq!(engine.compile_with_gates(5, 3), Ok(1));
    stop(&mut engine, original(), ExitReason::Gate { id: 17 }, 3);
    let outer = engine
        .capture_call(KEY, 1, CallingConvention32::Stdcall, 2)
        .unwrap();
    assert_eq!(outer.token, 1);
    let callback = engine
        .begin_callback(KEY, 1, outer.token, ENTRY, RETURN, 18, &[0x11, 0x22])
        .unwrap();
    assert_eq!(callback.token, 2);
    word(&mut engine, 0x8000, TAIL);
    let mut state = decode_state(&engine.arena()[..56]).unwrap();
    state.eip = TARGET;
    state.registers[4] = 0x8000;
    stop(&mut engine, state, ExitReason::NeedCode, 7);
    engine.arena_mut().unwrap()[100..140].fill(0x5a);
    engine
}

fn wire(generation: u32) -> [u8; 64] {
    let mut output = [0; 64];
    output[..16].copy_from_slice(&[0x52, 0x33, 0x43, 0x42, 1, 0, 1, 0, 64, 0, 0, 0, 0, 0, 0, 0]);
    for (index, value) in [
        2u32, 1, 1, 0, ENTRY, 0x8004, RETURN, 18, 2, 0, generation, 0,
    ]
    .into_iter()
    .enumerate()
    {
        output[16 + index * 4..20 + index * 4].copy_from_slice(&value.to_le_bytes());
    }
    output
}

fn error(error: CallError) -> HostError {
    HostError::Call(error)
}

fn unchanged<T>(
    engine: &mut EngineInstance,
    expected: HostError,
    operation: impl FnOnce(&mut EngineInstance) -> Result<T, HostError>,
) {
    let arena = engine.arena().to_vec();
    let artifact = engine.artifact_bytes().map(|value| value.to_vec());
    let generation = engine.generation();
    let stack = bytes(engine, 0x8000, 32);
    let snapshots: Vec<_> = [OUTER, TARGET, NEXT, 0x8000]
        .into_iter()
        .filter_map(|address| {
            engine
                .memory()
                .unwrap()
                .snapshot_code(GuestAddress(address), 1)
                .ok()
        })
        .collect();
    assert_eq!(operation(engine).err(), Some(expected));
    assert_eq!(engine.arena(), arena);
    assert_eq!(
        engine.artifact_bytes().map(|value| value.to_vec()),
        artifact
    );
    assert_eq!(engine.generation(), generation);
    assert_eq!(bytes(engine, 0x8000, 32), stack);
    for snapshot in snapshots {
        assert!(engine.memory().unwrap().is_code_current(&snapshot));
    }
}

#[test]
fn replacement_preserves_stopped_state_and_historical_entry_without_replaying_stack_setup() {
    let mut engine = active();
    replacement(&mut engine, TARGET, 6);
    let before = engine.arena().to_vec();
    let old_artifact = engine.artifact_bytes().unwrap().to_vec();
    let snapshots = [OUTER, TARGET, NEXT, 0x8000].map(|address| {
        engine
            .memory()
            .unwrap()
            .snapshot_code(GuestAddress(address), 1)
            .unwrap()
    });
    let mappings = engine.memory().unwrap().mapped_pages();
    assert_eq!(engine.resume_callback_code(KEY, 1, 2, 4, 2), Ok(2));
    let mut expected = before;
    expected[140..204].copy_from_slice(&wire(2));
    assert_eq!(engine.arena(), expected);
    assert_eq!(engine.key(), KEY);
    assert_eq!(engine.generation(), 2);
    assert_ne!(engine.artifact_bytes().unwrap(), old_artifact);
    assert_eq!(
        bytes(&engine, 0x8000, 32),
        [
            5, 0x11, 0, 0, 0, 0x12, 0, 0, 0x11, 0, 0, 0, 0x22, 0, 0, 0, 0, 0x20, 0, 0, 0x98, 0xba,
            0xdc, 0xfe, 0x10, 0x32, 0x54, 0x76, 0, 0, 0, 0,
        ]
    );
    assert_eq!(engine.memory().unwrap().mapped_pages(), mappings);
    for snapshot in snapshots {
        assert!(engine.memory().unwrap().is_code_current(&snapshot));
    }
    assert_eq!(engine.guard(KEY, 1), Err(HostError::InvalidArtifact));
    engine.guard(KEY, 2).unwrap();
}

fn outer_stop_bytes() -> [u8; 96] {
    let mut output = [0; 96];
    encode_state(&original(), &mut output[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 3,
            reason: ExitReason::Gate { id: 17 },
        },
        &mut output[56..],
    )
    .unwrap();
    output
}

fn callback_return(engine: &mut EngineInstance, result: u32) {
    let mut state = decode_state(&engine.arena()[..56]).unwrap();
    state.eip = RETURN;
    state.registers[0] = result;
    state.registers[4] = 0x8010;
    state.eflags = 2;
    stop(engine, state, ExitReason::Gate { id: 18 }, 11);
}

fn assert_outer_complete(engine: &EngineInstance, value: u32) {
    let mut expected = original();
    expected.registers[0] = value;
    expected.registers[4] = 0x801c;
    expected.eip = 0x2000;
    assert_eq!(decode_state(&engine.arena()[..56]).unwrap(), expected);
    assert_eq!(
        decode_exit(&engine.arena()[56..96]).unwrap(),
        ExecutionExit {
            retired: 0,
            reason: ExitReason::NeedCode
        }
    );
    assert_eq!(&engine.arena()[60..62], &[3, 0]);
}

#[test]
fn migrated_finish_restores_captured_outer_frame_and_does_not_spend_call_tokens() {
    let mut engine = active();
    replacement(&mut engine, TARGET, 6);
    engine.resume_callback_code(KEY, 1, 2, 4, 2).unwrap();
    word(&mut engine, 0x8010, 0xdead_beef);
    word(&mut engine, 0x8014, 0);
    engine.arena_mut().unwrap()[140..252].fill(0xee);
    callback_return(&mut engine, 0x7654_3210);
    unchanged(&mut engine, HostError::InvalidArtifact, |engine| {
        engine.finish_callback(KEY, 1, 2)
    });
    let returned = engine.finish_callback(KEY, 2, 2).unwrap();
    assert_eq!(
        (
            returned.token,
            returned.outer_token,
            returned.phase,
            returned.result,
            returned.generation
        ),
        (2, 1, 2, 0x7654_3210, 2)
    );
    assert_eq!((returned.entry_pc, returned.entry_esp), (ENTRY, 0x8004));
    assert_eq!(&engine.arena()[..96], &outer_stop_bytes());
    unchanged(&mut engine, error(CallError::InvalidRequest), |engine| {
        engine.begin_callback(KEY, 2, 1, ENTRY, RETURN, 18, &[])
    });
    unchanged(&mut engine, HostError::InvalidArtifact, |engine| {
        engine.complete_call(KEY, 1, 1, 77)
    });
    engine.complete_call(KEY, 2, 1, 77).unwrap();
    assert_outer_complete(&engine, 77);
    stop(&mut engine, original(), ExitReason::Gate { id: 17 }, 0);
    assert_eq!(
        engine
            .capture_call(KEY, 2, CallingConvention32::Cdecl, 0)
            .unwrap()
            .token,
        3
    );
}

#[test]
fn migrated_abort_restores_outer_generation_while_preserving_non_cpu_bytes() {
    let mut engine = active();
    replacement(&mut engine, TARGET, 6);
    engine.resume_callback_code(KEY, 1, 2, 4, 2).unwrap();
    engine.arena_mut().unwrap()[..96].fill(0xff);
    engine.arena_mut().unwrap()[96..100].copy_from_slice(&1u32.to_le_bytes());
    engine.arena_mut().unwrap()[140..204].fill(0xdd);
    let before = engine.arena().to_vec();
    let stack = bytes(&engine, 0x8000, 32);
    engine.abort_callback(KEY, 2).unwrap();
    let mut expected = before;
    expected[..96].copy_from_slice(&outer_stop_bytes());
    assert_eq!(engine.arena(), expected);
    assert_eq!(bytes(&engine, 0x8000, 32), stack);
    unchanged(&mut engine, error(CallError::InvalidToken), |engine| {
        engine.resume_callback_code(KEY, 2, 2, 0, 0)
    });
    unchanged(&mut engine, HostError::InvalidArtifact, |engine| {
        engine.complete_call(KEY, 1, 1, 8)
    });
    unchanged(&mut engine, error(CallError::Cancelled), |engine| {
        engine.complete_call(KEY, 2, 1, 8)
    });
    engine.arena_mut().unwrap()[96..100].fill(0);
    engine.complete_call(KEY, 2, 1, 8).unwrap();
    assert_outer_complete(&engine, 8);
}

#[test]
fn successive_replacements_change_only_generation_and_keep_the_same_continuation() {
    let mut engine = active();
    replacement(&mut engine, TARGET, 6);
    assert_eq!(engine.resume_callback_code(KEY, 1, 2, 4, 2), Ok(2));
    let mut state = decode_state(&engine.arena()[..56]).unwrap();
    state.eip = NEXT;
    state.registers[3] = 0x0123_4567;
    state.eflags = 0x457;
    stop(&mut engine, state, ExitReason::NeedCode, 9);
    replacement(&mut engine, NEXT, 2);
    let before = engine.arena().to_vec();
    let stack = bytes(&engine, 0x8000, 32);
    assert_eq!(engine.resume_callback_code(KEY, 2, 2, 4, 2), Ok(3));
    let mut expected = before;
    expected[140..204].copy_from_slice(&wire(3));
    assert_eq!(engine.arena(), expected);
    assert_eq!(bytes(&engine, 0x8000, 32), stack);
    for generation in [1, 2] {
        assert_eq!(
            engine.guard(KEY, generation),
            Err(HostError::InvalidArtifact)
        );
    }
    engine.guard(KEY, 3).unwrap();
    callback_return(&mut engine, 9);
    assert_eq!(engine.finish_callback(KEY, 3, 2).unwrap().generation, 3);
    engine.complete_call(KEY, 3, 1, 10).unwrap();
    assert_outer_complete(&engine, 10);
    stop(&mut engine, original(), ExitReason::Gate { id: 17 }, 0);
    assert_eq!(
        engine
            .capture_call(KEY, 3, CallingConvention32::Cdecl, 0)
            .unwrap()
            .token,
        3
    );
}

#[test]
fn identity_and_callback_token_precede_inner_pending_and_descriptor_validation() {
    let mut engine = active();
    replacement(&mut engine, TARGET, 6);
    for (key, generation) in [(KEY ^ 1, 1), (KEY, 0), (KEY, 2)] {
        unchanged(&mut engine, HostError::InvalidArtifact, |engine| {
            engine.resume_callback_code(key, generation, 0, 0, u32::MAX)
        });
    }
    for token in [0, 1, 3, u32::MAX] {
        unchanged(&mut engine, error(CallError::InvalidToken), |engine| {
            engine.resume_callback_code(KEY, 1, token, 0, 0)
        });
    }
    let stopped = engine.arena()[..96].to_vec();
    word(&mut engine, 0x7ff0, TAIL);
    let mut state = original();
    state.eip = INNER;
    state.registers[4] = 0x7ff0;
    stop(&mut engine, state, ExitReason::Gate { id: 19 }, 0);
    let inner = engine
        .capture_call(KEY, 1, CallingConvention32::Cdecl, 0)
        .unwrap();
    assert_eq!(inner.token, 3);
    unchanged(&mut engine, error(CallError::InvalidToken), |engine| {
        engine.resume_callback_code(KEY, 1, 0, 0, 0)
    });
    unchanged(&mut engine, error(CallError::Busy), |engine| {
        engine.resume_callback_code(KEY, 1, 2, 0, u32::MAX)
    });
    engine.arena_mut().unwrap()[0] = 0;
    engine.arena_mut().unwrap()[96] = 1;
    unchanged(&mut engine, error(CallError::Busy), |engine| {
        engine.resume_callback_code(KEY, 1, 2, 4, 2)
    });
    engine.abandon_call(KEY, inner.token).unwrap();
    engine.arena_mut().unwrap()[..96].copy_from_slice(&stopped);
    engine.arena_mut().unwrap()[96..100].fill(0);
    replacement(&mut engine, TARGET, 6);
    assert_eq!(engine.resume_callback_code(KEY, 1, 2, 4, 2), Ok(2));
}

#[test]
fn descriptor_counts_precede_bad_stop_and_cancellation_without_spending_generation() {
    let mut engine = active();
    replacement(&mut engine, TARGET, 6);
    let stopped = engine.arena()[..96].to_vec();
    engine.arena_mut().unwrap()[0] = 0;
    engine.arena_mut().unwrap()[56] = 0;
    engine.arena_mut().unwrap()[96..100].copy_from_slice(&0x8000_0000u32.to_le_bytes());
    for (count, gates) in [(0, 0), (9, 0), (1, 2), (u32::MAX, 0), (4, u32::MAX)] {
        unchanged(&mut engine, HostError::InvalidRequest, |engine| {
            engine.resume_callback_code(KEY, 1, 2, count, gates)
        });
    }
    unchanged(&mut engine, error(CallError::InvalidStop), |engine| {
        engine.resume_callback_code(KEY, 1, 2, 4, 2)
    });
    engine.arena_mut().unwrap()[..96].copy_from_slice(&stopped);
    engine.arena_mut().unwrap()[140..172].fill(0);
    unchanged(&mut engine, error(CallError::Cancelled), |engine| {
        engine.resume_callback_code(KEY, 1, 2, 4, 2)
    });
    engine.arena_mut().unwrap()[96..100].fill(0);
    replacement(&mut engine, TARGET, 6);
    assert_eq!(engine.resume_callback_code(KEY, 1, 2, 4, 2), Ok(2));
}

#[test]
fn installation_requires_canonical_state_and_literal_v3_need_code() {
    let mut engine = active();
    replacement(&mut engine, TARGET, 6);
    let stopped = engine.arena()[..96].to_vec();
    for version in [1u16, 2] {
        engine.arena_mut().unwrap()[60..62].copy_from_slice(&version.to_le_bytes());
        assert_eq!(
            decode_exit(&engine.arena()[56..96]).unwrap().reason,
            ExitReason::NeedCode
        );
        unchanged(&mut engine, error(CallError::InvalidStop), |engine| {
            engine.resume_callback_code(KEY, 1, 2, 4, 2)
        });
        engine.arena_mut().unwrap()[..96].copy_from_slice(&stopped);
    }
    for (offset, value) in [(0, 0), (4, 2), (52, 0), (56, 0), (64, 39), (68, 1), (80, 1)] {
        engine.arena_mut().unwrap()[offset] = value;
        unchanged(&mut engine, error(CallError::InvalidStop), |engine| {
            engine.resume_callback_code(KEY, 1, 2, 4, 2)
        });
        engine.arena_mut().unwrap()[..96].copy_from_slice(&stopped);
    }
    for reason in [
        ExitReason::Budget,
        ExitReason::Cancelled,
        ExitReason::Gate { id: 17 },
    ] {
        let state = decode_state(&stopped[..56]).unwrap();
        stop(&mut engine, state, reason, 7);
        engine.arena_mut().unwrap()[96] = 1;
        unchanged(&mut engine, error(CallError::InvalidStop), |engine| {
            engine.resume_callback_code(KEY, 1, 2, 4, 2)
        });
        engine.arena_mut().unwrap()[96..100].fill(0);
    }
    engine.arena_mut().unwrap()[..96].copy_from_slice(&stopped);
    assert_eq!(engine.resume_callback_code(KEY, 1, 2, 4, 2), Ok(2));
}

#[test]
fn structural_decode_and_fetch_compile_errors_are_typed_atomic_and_retryable() {
    let mut engine = active();
    descriptors(&mut engine, &[(TARGET, 0)], &[]);
    unchanged(
        &mut engine,
        HostError::Compile(CompileError::InvalidBlocks),
        |engine| engine.resume_callback_code(KEY, 1, 2, 1, 0),
    );
    descriptors(
        &mut engine,
        &[(OUTER, 2), (RETURN, 2), (TARGET, 6)],
        &[(OUTER, 17), (RETURN, 17)],
    );
    unchanged(
        &mut engine,
        HostError::Compile(CompileError::InvalidGates),
        |engine| engine.resume_callback_code(KEY, 1, 2, 3, 2),
    );
    upload(&mut engine, TARGET, &[0xf4]);
    descriptors(&mut engine, &[(TARGET, 1)], &[]);
    unchanged(
        &mut engine,
        HostError::Compile(CompileError::Instruction {
            pc: GuestAddress(TARGET),
            cause: InstructionError::Decode(DecodeError::Unsupported(
                UnsupportedFeature::Privileged,
            )),
        }),
        |engine| engine.resume_callback_code(KEY, 1, 2, 1, 0),
    );
    upload(&mut engine, TARGET, &[0xb8, 0x78, 0x56, 0x34, 0x12, 0xc3]);
    engine.protect(TARGET, 1, 1).unwrap();
    replacement(&mut engine, TARGET, 6);
    unchanged(
        &mut engine,
        HostError::Compile(CompileError::Instruction {
            pc: GuestAddress(TARGET),
            cause: InstructionError::Decode(DecodeError::MemoryFault {
                pc: GuestAddress(TARGET),
                fault: MemoryFault {
                    address: GuestAddress(TARGET),
                    access: Access::Execute,
                    reason: FaultReason::Permission,
                },
                length: 1,
            }),
        }),
        |engine| engine.resume_callback_code(KEY, 1, 2, 4, 2),
    );
    engine.protect(TARGET, 1, 7).unwrap();
    replacement(&mut engine, TARGET, 6);
    assert_eq!(engine.resume_callback_code(KEY, 1, 2, 4, 2), Ok(2));
}

#[test]
fn stopped_target_must_be_an_ordinary_instruction_boundary_in_replacement() {
    for case in 0..3 {
        let mut engine = active();
        match case {
            0 => descriptors(
                &mut engine,
                &[(OUTER, 2), (RETURN, 2), (TAIL, 4)],
                &[(OUTER, 17), (RETURN, 18)],
            ),
            1 => {
                upload(&mut engine, TARGET, &[0x0f, 0x0b]);
                descriptors(
                    &mut engine,
                    &[(OUTER, 2), (RETURN, 2), (TARGET, 2)],
                    &[(OUTER, 17), (RETURN, 18), (TARGET, 20)],
                );
            }
            2 => {
                engine.map(0x3000, 1, 7).unwrap();
                upload(
                    &mut engine,
                    TARGET - 1,
                    &[0xb8, 0x78, 0x56, 0x34, 0x12, 0xc3],
                );
                descriptors(
                    &mut engine,
                    &[(OUTER, 2), (RETURN, 2), (TARGET - 1, 6)],
                    &[(OUTER, 17), (RETURN, 18)],
                );
            }
            _ => unreachable!(),
        }
        let gate_count = if case == 1 { 3 } else { 2 };
        unchanged(&mut engine, HostError::InvalidRequest, |engine| {
            engine.resume_callback_code(KEY, 1, 2, 3, gate_count)
        });
        upload(&mut engine, TARGET, &[0xb8, 0x78, 0x56, 0x34, 0x12, 0xc3]);
        replacement(&mut engine, TARGET, 6);
        assert_eq!(
            engine.resume_callback_code(KEY, 1, 2, 4, 2),
            Ok(2),
            "case {case}"
        );
    }
}

#[test]
fn replacement_bindings_come_from_private_outer_and_callback_authority() {
    let mut engine = active();
    for (blocks, gates) in [
        (vec![(RETURN, 2), (TARGET, 6)], vec![(RETURN, 18)]),
        (vec![(OUTER, 2), (TARGET, 6)], vec![(OUTER, 17)]),
        (
            vec![(OUTER, 2), (RETURN, 2), (TARGET, 6)],
            vec![(OUTER, 20), (RETURN, 18)],
        ),
        (
            vec![(OUTER, 2), (RETURN, 2), (TARGET, 6)],
            vec![(OUTER, 17), (RETURN, 20)],
        ),
        (
            vec![(OUTER, 2), (RETURN, 2), (TARGET, 6)],
            vec![(OUTER, 18), (RETURN, 17)],
        ),
    ] {
        descriptors(&mut engine, &blocks, &gates);
        unchanged(&mut engine, HostError::InvalidRequest, |engine| {
            engine.resume_callback_code(KEY, 1, 2, blocks.len() as u32, gates.len() as u32)
        });
    }
    replacement(&mut engine, TARGET, 6);
    assert_eq!(engine.resume_callback_code(KEY, 1, 2, 4, 2), Ok(2));
    callback_return(&mut engine, 0);
    engine.finish_callback(KEY, 2, 2).unwrap();
    assert_eq!(&engine.arena()[..96], &outer_stop_bytes());
}

#[test]
fn stale_old_code_and_closed_lifecycle_reject_before_lower_priority_inputs() {
    let mut engine = active();
    upload(&mut engine, OUTER, &[0x0f, 0x0b]);
    replacement(&mut engine, TARGET, 6);
    unchanged(&mut engine, HostError::InvalidArtifact, |engine| {
        engine.resume_callback_code(KEY ^ 1, 1, 0, 0, 0)
    });
    unchanged(&mut engine, HostError::CodeInvalidated, |engine| {
        engine.resume_callback_code(KEY, 1, 0, 0, u32::MAX)
    });
    engine.abort_callback(KEY, 2).unwrap();
    engine.abandon_call(KEY, 1).unwrap();
    replacement(&mut engine, TARGET, 6);
    assert_eq!(engine.compile_with_gates(4, 2), Ok(2));
    unchanged(&mut engine, error(CallError::InvalidToken), |engine| {
        engine.resume_callback_code(KEY, 2, 2, 4, 2)
    });
    engine.close();
    let arena = engine.arena().to_vec();
    assert_eq!(
        engine.resume_callback_code(0, 0, 0, 0, u32::MAX),
        Err(HostError::Closed)
    );
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.generation(), 0);
}

#[test]
fn inner_calls_after_install_use_new_generation_and_shared_unspent_token() {
    let mut engine = active();
    descriptors(
        &mut engine,
        &[(OUTER, 2), (RETURN, 2), (TARGET, 6), (TAIL, 4), (INNER, 2)],
        &[(OUTER, 17), (RETURN, 18), (INNER, 19)],
    );
    assert_eq!(engine.resume_callback_code(KEY, 1, 2, 5, 3), Ok(2));
    word(&mut engine, 0x7ff0, TAIL);
    let mut state = original();
    state.eip = INNER;
    state.registers[4] = 0x7ff0;
    stop(&mut engine, state, ExitReason::Gate { id: 19 }, 2);
    unchanged(&mut engine, HostError::InvalidArtifact, |engine| {
        engine.capture_call(KEY, 1, CallingConvention32::Cdecl, 0)
    });
    let inner = engine
        .capture_call(KEY, 2, CallingConvention32::Cdecl, 0)
        .unwrap();
    assert_eq!(
        (inner.token, inner.gate_pc, inner.return_pc),
        (3, INNER, TAIL)
    );
    unchanged(&mut engine, error(CallError::Busy), |engine| {
        engine.resume_callback_code(KEY, 2, 2, 5, 3)
    });
    unchanged(&mut engine, error(CallError::Busy), |engine| {
        engine.complete_call(KEY, 2, 1, 0)
    });
    engine.complete_call(KEY, 2, 3, 0xaabb_ccdd).unwrap();
    let inner_result = decode_state(&engine.arena()[..56]).unwrap();
    assert_eq!(
        (
            inner_result.eip,
            inner_result.registers[4],
            inner_result.registers[0]
        ),
        (TAIL, 0x7ff4, 0xaabb_ccdd)
    );
    callback_return(&mut engine, 99);
    engine.finish_callback(KEY, 2, 2).unwrap();
    engine.complete_call(KEY, 2, 1, 100).unwrap();
    assert_outer_complete(&engine, 100);
}
