use ring3_engine::abi::x86::{decode_state, encode_exit_v3, encode_state};
use ring3_engine::cpu::x86::State32;
use ring3_engine::cpu::{ExecutionExit, ExitReason};
use ring3_engine::memory::{Access, FaultReason, GuestAddress, MemoryError, MemoryFault};
use ring3_engine::process::{CallError, EngineInstance, HostError};
use ring3_engine::windows::CallingConvention32;

const KEY: u64 = 0xf123_4567_89ab_cdef;
const GATE: u32 = 0x1000;
const ID: u32 = 17;
const RETURN: u32 = 0x1100;

fn upload(engine: &mut EngineInstance, address: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
    engine.upload(address, bytes.len() as u32).unwrap();
}

fn engine(key: u64) -> EngineInstance {
    let mut engine = EngineInstance::new(4, key).unwrap();
    engine.map(GATE, 1, 7).unwrap();
    upload(&mut engine, GATE, &[0x0f, 0x0b]);
    engine.arena_mut().unwrap()[140..156]
        .copy_from_slice(&[0, 0x10, 0, 0, 2, 0, 0, 0, 0, 0x10, 0, 0, 17, 0, 0, 0]);
    assert_eq!(engine.compile_with_gates(1, 1), Ok(1));
    engine
}

fn word(engine: &mut EngineInstance, address: u32, value: u32) {
    engine.write32(address, value).unwrap();
}

fn guest_word(engine: &EngineInstance, address: u32) -> u32 {
    let mut bytes = [0; 4];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(address), &mut bytes)
        .unwrap();
    u32::from_le_bytes(bytes)
}

fn inject_gate_stop(engine: &mut EngineInstance, esp: u32) -> State32 {
    // native injection supplies the already-tested CPU stop, not a translated execution.
    let state = State32 {
        registers: [
            0x89ab_cdef,
            0x1357_9bdf,
            0x2345_6789,
            0x3456_789a,
            esp,
            0x5678_9abc,
            0x6789_abcd,
            0x789a_bcde,
        ],
        eip: GATE,
        eflags: 0xcd7,
    };
    let arena = engine.arena_mut().unwrap();
    encode_state(&state, &mut arena[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 3,
            reason: ExitReason::Gate { id: ID },
        },
        &mut arena[56..96],
    )
    .unwrap();
    arena[96..100].fill(0);
    arena[100..140].fill(0x5a);
    arena[140..].fill(0xcc);
    state
}

fn ready(key: u64) -> (EngineInstance, State32) {
    let mut engine = engine(key);
    engine.map(0x8000, 1, 3).unwrap();
    for (address, value) in [
        (0x8000, RETURN),
        (0x8004, 0xfedc_ba98),
        (0x8008, 0x7654_3210),
    ] {
        word(&mut engine, address, value);
    }
    let state = inject_gate_stop(&mut engine, 0x8000);
    (engine, state)
}

fn call_error(error: CallError) -> HostError {
    HostError::Call(error)
}

fn unchanged<T>(
    engine: &mut EngineInstance,
    expected: HostError,
    operation: impl FnOnce(&mut EngineInstance) -> Result<T, HostError>,
) {
    let before = engine.arena().to_vec();
    let generation = engine.generation();
    let artifact = engine.artifact_bytes().map(|bytes| bytes.to_vec());
    assert_eq!(operation(engine).err(), Some(expected));
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.generation(), generation);
    assert_eq!(
        engine.artifact_bytes().map(|bytes| bytes.to_vec()),
        artifact
    );
}

fn expected_completion(before: &[u8], esp: u32, return_pc: u32, result: u32) -> Vec<u8> {
    let mut expected = before.to_vec();
    expected[16..20].copy_from_slice(&result.to_le_bytes());
    expected[32..36].copy_from_slice(&esp.to_le_bytes());
    expected[48..52].copy_from_slice(&return_pc.to_le_bytes());
    expected[56..96].copy_from_slice(&[
        0x52, 0x33, 0x45, 0x58, 3, 0, 1, 0, 40, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ]);
    expected
}

#[test]
fn bound_gate_capture_publishes_checked_words_and_the_first_token() {
    let (mut engine, state) = ready(KEY);
    let before = engine.arena().to_vec();
    let artifact = engine.artifact_bytes().unwrap().to_vec();
    let record = engine
        .capture_call(KEY, 1, CallingConvention32::Cdecl, 2)
        .unwrap();
    assert_eq!(record.token, 1);
    assert_eq!(record.id, ID);
    assert_eq!(record.convention, 1);
    assert_eq!(record.stack_words, 2);
    assert_eq!(record.gate_pc, GATE);
    assert_eq!(record.entry_esp, 0x8000);
    assert_eq!(record.return_pc, RETURN);
    assert_eq!(record.this_pointer, 0);
    assert_eq!(&record.arguments[..2], &[0xfedc_ba98, 0x7654_3210]);
    assert_eq!(&record.arguments[2..], &[0; 14]);
    let mut wire = [0; 112];
    wire[..16].copy_from_slice(&[0x52, 0x33, 0x43, 0x46, 1, 0, 1, 0, 112, 0, 0, 0, 0, 0, 0, 0]);
    for (index, value) in [
        1u32,
        17,
        1,
        2,
        0x1000,
        0x8000,
        0x1100,
        0,
        0xfedc_ba98,
        0x7654_3210,
    ]
    .into_iter()
    .enumerate()
    {
        wire[16 + index * 4..20 + index * 4].copy_from_slice(&value.to_le_bytes());
    }
    let mut expected = before;
    expected[140..252].copy_from_slice(&wire);
    assert_eq!(engine.arena(), expected);
    assert_eq!(decode_state(&engine.arena()[..56]), Ok(state));
    assert_eq!(engine.artifact_bytes().unwrap(), artifact);
    assert_eq!(engine.generation(), 1);
    assert_eq!(guest_word(&engine, 0x8000), RETURN);
    assert_eq!(guest_word(&engine, 0x8004), 0xfedc_ba98);
    assert_eq!(guest_word(&engine, 0x8008), 0x7654_3210);
}

#[test]
fn process_capture_accepts_the_full_fixed_record_then_advances_only_on_success() {
    let (mut engine, _) = ready(KEY);
    for index in 0..16 {
        word(&mut engine, 0x8004 + index * 4, 0xa000_0000 + index);
    }
    inject_gate_stop(&mut engine, 0x8000);
    let full = engine
        .capture_call(KEY, 1, CallingConvention32::Thiscall, 16)
        .unwrap();
    assert_eq!(full.token, 1);
    assert_eq!(full.stack_words, 16);
    assert_eq!(full.convention, 3);
    assert_eq!(full.this_pointer, 0x1357_9bdf);
    assert_eq!(
        full.arguments,
        std::array::from_fn(|index| 0xa000_0000 + index as u32)
    );
    let before = engine.arena().to_vec();
    engine.abandon_call(KEY, full.token).unwrap();
    assert_eq!(engine.arena(), before);
    let empty = engine
        .capture_call(KEY, 1, CallingConvention32::Cdecl, 0)
        .unwrap();
    assert_eq!(empty.token, 2);
    assert_eq!(empty.return_pc, RETURN);
    assert_eq!(empty.stack_words, 0);
    assert_eq!(empty.this_pointer, 0);
    assert_eq!(empty.arguments, [0; 16]);
}

#[test]
fn session_and_generation_precede_invalid_ingress_or_stop_bytes() {
    let (mut engine, _) = ready(KEY);
    engine.arena_mut().unwrap()[0] = 0;
    engine.arena_mut().unwrap()[96..100].copy_from_slice(&1u32.to_le_bytes());
    for (key, generation) in [(KEY ^ 1, 1), (KEY, 0), (KEY, 2)] {
        unchanged(&mut engine, HostError::InvalidArtifact, |engine| {
            engine.capture_call_raw(key, generation, u32::MAX, u32::MAX)
        });
    }
    inject_gate_stop(&mut engine, 0x8000);
    assert_eq!(
        engine
            .capture_call(KEY, 1, CallingConvention32::Cdecl, 0)
            .unwrap()
            .token,
        1
    );
}

#[test]
fn invalid_counts_and_raw_tags_precede_stop_decoding_and_leave_token_unspent() {
    let (mut engine, _) = ready(KEY);
    engine.arena_mut().unwrap()[0] = 0;
    for count in [17, u32::MAX] {
        unchanged(
            &mut engine,
            call_error(CallError::InvalidRequest),
            |engine| engine.capture_call(KEY, 1, CallingConvention32::Cdecl, count),
        );
    }
    for tag in [0, 4, u32::MAX] {
        unchanged(
            &mut engine,
            call_error(CallError::InvalidRequest),
            |engine| engine.capture_call_raw(KEY, 1, tag, 0),
        );
    }
    inject_gate_stop(&mut engine, 0x8000);
    assert_eq!(engine.capture_call_raw(KEY, 1, 2, 1).unwrap().token, 1);
}

#[test]
fn capture_requires_canonical_state_exit_and_the_current_exact_gate_binding() {
    for change in 0..9 {
        let (mut engine, _) = ready(KEY);
        let arena = engine.arena_mut().unwrap();
        match change {
            0 => arena[0] = 0,
            1 => arena[4] = 3,
            2 => arena[52..56].copy_from_slice(&0u32.to_le_bytes()),
            3 => arena[56] = 0,
            4 => arena[60] = 2,
            5 => {
                arena[72..76].copy_from_slice(&1u32.to_le_bytes());
                arena[80..84].fill(0);
            }
            6 => arena[80..84].copy_from_slice(&18u32.to_le_bytes()),
            7 => arena[48..52].copy_from_slice(&(GATE + 1).to_le_bytes()),
            _ => arena[48..52].copy_from_slice(&0x2000u32.to_le_bytes()),
        }
        arena[96..100].copy_from_slice(&1u32.to_le_bytes());
        unchanged(&mut engine, call_error(CallError::InvalidStop), |engine| {
            engine.capture_call(KEY, 1, CallingConvention32::Cdecl, 2)
        });
        assert_eq!(guest_word(&engine, 0x8000), RETURN);
        inject_gate_stop(&mut engine, 0x8000);
        assert_eq!(
            engine
                .capture_call(KEY, 1, CallingConvention32::Cdecl, 0)
                .unwrap()
                .token,
            1
        );
    }
}

#[test]
fn cancelled_capture_precedes_guest_reads_and_can_retry_without_spending_a_token() {
    let mut engine = engine(KEY);
    inject_gate_stop(&mut engine, 0x9000);
    engine.arena_mut().unwrap()[96..100].copy_from_slice(&1u32.to_le_bytes());
    unchanged(&mut engine, call_error(CallError::Cancelled), |engine| {
        engine.capture_call(KEY, 1, CallingConvention32::Cdecl, 0)
    });
    engine.map(0x9000, 1, 3).unwrap();
    word(&mut engine, 0x9000, RETURN);
    engine.arena_mut().unwrap()[96..100].fill(0);
    assert_eq!(
        engine
            .capture_call(KEY, 1, CallingConvention32::Cdecl, 0)
            .unwrap()
            .token,
        1
    );
}

#[test]
fn capture_word_faults_preserve_all_outputs_pending_state_and_code() {
    let cases = [
        (0x9000, 2, 0x9000, FaultReason::Unmapped),
        (0x8ffc, 1, 0x8ffc, FaultReason::Permission),
        (0x8ffe, 0, 0x9000, FaultReason::Unmapped),
        (0x8ff8, 2, 0x9000, FaultReason::Unmapped),
        (0xffff_fffc, 1, 0, FaultReason::Unmapped),
        (0xffff_fffd, 1, 0xffff_fffd, FaultReason::AddressOverflow),
        (0xffff_fff9, 1, 0xffff_fffd, FaultReason::AddressOverflow),
    ];
    for (index, (esp, count, address, reason)) in cases.into_iter().enumerate() {
        let mut engine = engine(KEY);
        match index {
            1 => {
                engine.map(0x8000, 1, 3).unwrap();
                word(&mut engine, 0x8ffc, RETURN);
                engine.protect(0x8000, 1, 2).unwrap();
            }
            2 => {
                engine.map(0x8000, 1, 3).unwrap();
                word(&mut engine, 0x8ffc, RETURN);
            }
            3 => {
                engine.map(0x8000, 1, 3).unwrap();
                word(&mut engine, 0x8ff8, RETURN);
                word(&mut engine, 0x8ffc, 0xfedc_ba98);
            }
            4 | 6 => {
                engine.map(0xffff_f000, 1, 3).unwrap();
                word(&mut engine, esp, RETURN);
            }
            _ => {}
        }
        inject_gate_stop(&mut engine, esp);
        let error = call_error(CallError::Memory(MemoryError::Fault(MemoryFault {
            address: GuestAddress(address),
            access: Access::Read,
            reason,
        })));
        unchanged(&mut engine, error, |engine| {
            engine.capture_call(KEY, 1, CallingConvention32::Stdcall, count)
        });
        engine.guard(KEY, 1).unwrap();
        if matches!(index, 3 | 4 | 6) {
            assert_eq!(guest_word(&engine, esp), RETURN);
        }
        if index == 3 {
            assert_eq!(guest_word(&engine, 0x8ffc), 0xfedc_ba98);
        }
    }
}

#[test]
fn successful_completion_uses_private_captured_values_and_preserves_the_rest() {
    for (convention, tag, final_esp, this_pointer) in [
        (CallingConvention32::Cdecl, 1, 0x8004, 0),
        (CallingConvention32::Stdcall, 2, 0x800c, 0),
        (CallingConvention32::Thiscall, 3, 0x800c, 0x1357_9bdf),
    ] {
        let (mut engine, mut expected_state) = ready(KEY);
        let record = engine.capture_call(KEY, 1, convention, 2).unwrap();
        assert_eq!(record.convention, tag);
        assert_eq!(record.this_pointer, this_pointer);
        word(&mut engine, 0x8000, 0xdead_beef);
        word(&mut engine, 0x8004, 0);
        word(&mut engine, 0x8008, u32::MAX);
        engine.unmap(0x8000, 1).unwrap();
        engine.arena_mut().unwrap()[140..252].fill(0xff);
        let before = engine.arena().to_vec();
        let expected = expected_completion(&before, final_esp, RETURN, 0xa1b2_c3d4);
        engine
            .complete_call(KEY, 1, record.token, 0xa1b2_c3d4)
            .unwrap();
        assert_eq!(engine.arena(), expected);
        expected_state.registers[0] = 0xa1b2_c3d4;
        expected_state.registers[4] = final_esp;
        expected_state.eip = RETURN;
        assert_eq!(decode_state(&engine.arena()[..56]), Ok(expected_state));
        assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
        engine.guard(KEY, 1).unwrap();
    }
}

#[test]
fn completion_tokens_are_single_use_and_the_consumed_gate_cannot_be_recaptured() {
    let (mut engine, _) = ready(KEY);
    word(&mut engine, 0x8000, GATE);
    inject_gate_stop(&mut engine, 0x8000);
    let first = engine
        .capture_call(KEY, 1, CallingConvention32::Cdecl, 0)
        .unwrap();
    engine.complete_call(KEY, 1, first.token, 41).unwrap();
    unchanged(&mut engine, call_error(CallError::InvalidToken), |engine| {
        engine.complete_call(KEY, 1, first.token, 99)
    });
    unchanged(&mut engine, call_error(CallError::InvalidStop), |engine| {
        engine.capture_call(KEY, 1, CallingConvention32::Cdecl, 0)
    });
    word(&mut engine, 0x8004, 0x2200);
    inject_gate_stop(&mut engine, 0x8004);
    let second = engine
        .capture_call(KEY, 1, CallingConvention32::Cdecl, 0)
        .unwrap();
    assert_eq!(second.token, 2);
    unchanged(&mut engine, call_error(CallError::InvalidToken), |engine| {
        engine.complete_call(KEY, 1, first.token, 99)
    });
    engine.complete_call(KEY, 1, second.token, 42).unwrap();
    assert_eq!(decode_state(&engine.arena()[..56]).unwrap().eip, 0x2200);
}

#[test]
fn failed_completion_identity_token_and_byte_checks_are_atomic_and_retryable() {
    let (mut engine, _) = ready(KEY);
    let record = engine
        .capture_call(KEY, 1, CallingConvention32::Cdecl, 2)
        .unwrap();
    for (key, generation, token, error) in [
        (KEY ^ 1, 1, record.token, HostError::InvalidArtifact),
        (KEY, 0, record.token, HostError::InvalidArtifact),
        (KEY, 2, record.token, HostError::InvalidArtifact),
        (KEY, 1, 0, call_error(CallError::InvalidToken)),
        (
            KEY,
            1,
            record.token + 1,
            call_error(CallError::InvalidToken),
        ),
    ] {
        unchanged(&mut engine, error, |engine| {
            engine.complete_call(key, generation, token, 7)
        });
    }
    let saved = engine.arena()[..96].to_vec();
    for offset in [0, 16, 56, 76] {
        engine.arena_mut().unwrap()[offset] ^= 1;
        unchanged(&mut engine, call_error(CallError::StateChanged), |engine| {
            engine.complete_call(KEY, 1, record.token, 7)
        });
        engine.arena_mut().unwrap()[..96].copy_from_slice(&saved);
    }
    engine.complete_call(KEY, 1, record.token, 7).unwrap();
    assert_eq!(decode_state(&engine.arena()[..56]).unwrap().registers[0], 7);
}

#[test]
fn cancellation_keeps_pending_for_retry_and_explicit_abandon_preserves_cpu_bytes() {
    let (mut engine, _) = ready(KEY);
    let record = engine
        .capture_call(KEY, 1, CallingConvention32::Stdcall, 2)
        .unwrap();
    engine.arena_mut().unwrap()[96..100].copy_from_slice(&1u32.to_le_bytes());
    unchanged(&mut engine, call_error(CallError::InvalidToken), |engine| {
        engine.complete_call(KEY, 1, record.token + 1, 7)
    });
    unchanged(&mut engine, call_error(CallError::Cancelled), |engine| {
        engine.complete_call(KEY, 1, record.token, 7)
    });
    engine.arena_mut().unwrap()[96..100].fill(0);
    engine.complete_call(KEY, 1, record.token, 7).unwrap();
    inject_gate_stop(&mut engine, 0x8000);
    let second = engine
        .capture_call(KEY, 1, CallingConvention32::Cdecl, 0)
        .unwrap();
    assert_eq!(second.token, 2);
    engine.arena_mut().unwrap()[96..100].copy_from_slice(&1u32.to_le_bytes());
    let before = engine.arena().to_vec();
    engine.abandon_call(KEY, second.token).unwrap();
    assert_eq!(engine.arena(), before);
    unchanged(&mut engine, call_error(CallError::Cancelled), |engine| {
        engine.capture_call(KEY, 1, CallingConvention32::Cdecl, 0)
    });
    engine.arena_mut().unwrap()[96..100].fill(0);
    assert_eq!(
        engine
            .capture_call(KEY, 1, CallingConvention32::Cdecl, 0)
            .unwrap()
            .token,
        3
    );
}

#[test]
fn pending_parks_guard_and_compile_before_invalid_counts_or_raw_tags() {
    let (mut engine, _) = ready(KEY);
    let record = engine
        .capture_call(KEY, 1, CallingConvention32::Cdecl, 0)
        .unwrap();
    unchanged(&mut engine, HostError::InvalidArtifact, |engine| {
        engine.guard(KEY ^ 1, 1)
    });
    unchanged(&mut engine, call_error(CallError::Busy), |engine| {
        engine.guard(KEY, 1)
    });
    unchanged(&mut engine, call_error(CallError::Busy), |engine| {
        engine.compile(0)
    });
    unchanged(&mut engine, call_error(CallError::Busy), |engine| {
        engine.compile_with_gates(0, u32::MAX)
    });
    unchanged(&mut engine, call_error(CallError::Busy), |engine| {
        engine.capture_call_raw(KEY, 1, u32::MAX, u32::MAX)
    });
    engine.abandon_call(KEY, record.token).unwrap();
    engine.guard(KEY, 1).unwrap();
}

#[test]
fn stale_code_precedes_pending_but_abandon_needs_no_current_code_state_or_cancel() {
    let (mut engine, _) = ready(KEY);
    let record = engine
        .capture_call(KEY, 1, CallingConvention32::Cdecl, 0)
        .unwrap();
    upload(&mut engine, GATE, &[0x0f, 0x0b]);
    engine.arena_mut().unwrap()[0] = 0;
    engine.arena_mut().unwrap()[96..100].copy_from_slice(&1u32.to_le_bytes());
    unchanged(&mut engine, HostError::CodeInvalidated, |engine| {
        engine.guard(KEY, 1)
    });
    unchanged(&mut engine, HostError::CodeInvalidated, |engine| {
        engine.capture_call_raw(KEY, 1, u32::MAX, u32::MAX)
    });
    unchanged(&mut engine, HostError::CodeInvalidated, |engine| {
        engine.complete_call(KEY, 1, 0, 7)
    });
    unchanged(&mut engine, HostError::InvalidArtifact, |engine| {
        engine.abandon_call(KEY ^ 1, record.token)
    });
    unchanged(&mut engine, call_error(CallError::InvalidToken), |engine| {
        engine.abandon_call(KEY, record.token + 1)
    });
    let before = engine.arena().to_vec();
    engine.abandon_call(KEY, record.token).unwrap();
    assert_eq!(engine.arena(), before);
    engine.arena_mut().unwrap()[140..156]
        .copy_from_slice(&[0, 0x10, 0, 0, 2, 0, 0, 0, 0, 0x10, 0, 0, 17, 0, 0, 0]);
    assert_eq!(engine.compile_with_gates(1, 1), Ok(2));
    unchanged(&mut engine, call_error(CallError::InvalidToken), |engine| {
        engine.complete_call(KEY, 2, record.token, 7)
    });
}

#[test]
fn close_clears_pending_and_keeps_the_arena_tombstone() {
    let (mut engine, _) = ready(KEY);
    let record = engine
        .capture_call(KEY, 1, CallingConvention32::Cdecl, 0)
        .unwrap();
    let before = engine.arena().to_vec();
    engine.close();
    assert_eq!(engine.arena(), before);
    unchanged(&mut engine, HostError::Closed, |engine| {
        engine.complete_call(KEY, 1, record.token, 7)
    });
    unchanged(&mut engine, HostError::Closed, |engine| {
        engine.abandon_call(KEY, record.token)
    });
    unchanged(&mut engine, HostError::Closed, |engine| {
        engine.capture_call_raw(KEY, 1, u32::MAX, u32::MAX)
    });
    unchanged(&mut engine, HostError::Closed, |engine| {
        engine.compile_with_gates(0, u32::MAX)
    });
}

#[test]
fn equal_token_and_generation_in_another_instance_do_not_authorize_completion() {
    let (mut first, _) = ready(KEY);
    let (mut second, _) = ready(KEY ^ 1);
    let a = first
        .capture_call(KEY, 1, CallingConvention32::Cdecl, 0)
        .unwrap();
    let b = second
        .capture_call(KEY ^ 1, 1, CallingConvention32::Cdecl, 0)
        .unwrap();
    assert_eq!(a.token, b.token);
    unchanged(&mut second, HostError::InvalidArtifact, |engine| {
        engine.complete_call(KEY, 1, a.token, 7)
    });
    unchanged(&mut second, HostError::InvalidArtifact, |engine| {
        engine.abandon_call(KEY, a.token)
    });
    second.complete_call(KEY ^ 1, 1, b.token, 7).unwrap();
    first.complete_call(KEY, 1, a.token, 8).unwrap();
    assert_eq!(decode_state(&first.arena()[..56]).unwrap().registers[0], 8);
    assert_eq!(decode_state(&second.arena()[..56]).unwrap().registers[0], 7);
}
