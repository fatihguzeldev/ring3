use ring3_engine::abi::x86::{decode_state, encode_exit_v3, encode_state};
use ring3_engine::cpu::x86::State32;
use ring3_engine::cpu::{ExecutionExit, ExitReason};
use ring3_engine::memory::{Access, FaultReason, GuestAddress, MemoryError, MemoryFault};
use ring3_engine::process::{CallError, EngineInstance, HostError};
use ring3_engine::windows::CallingConvention32;

const KEY: u64 = 0xfedc_ba98_1234_5678;
const OUTER: u32 = 0x1000;
const ENTRY: u32 = 0x1100;
const RETURN: u32 = 0x1200;
const INNER: u32 = 0x1300;

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

fn original(esp: u32) -> State32 {
    State32 {
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
        eip: OUTER,
        eflags: 0xcd7,
    }
}

fn compile(engine: &mut EngineInstance) -> u32 {
    for (index, value) in [
        OUTER, 2, ENTRY, 9, RETURN, 2, INNER, 2, OUTER, 17, RETURN, 18, INNER, 19,
    ]
    .into_iter()
    .enumerate()
    {
        engine.arena_mut().unwrap()[140 + index * 4..144 + index * 4]
            .copy_from_slice(&value.to_le_bytes());
    }
    engine.compile_with_gates(4, 3).unwrap()
}

fn fixture(esp: u32) -> EngineInstance {
    let mut engine = EngineInstance::new(6, KEY).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.map(0x7000, 2, 7).unwrap();
    if !(0x1000..0x9000).contains(&esp) {
        engine.map(esp & !0xfff, 1, 7).unwrap();
    }
    for address in [OUTER, RETURN, INNER] {
        upload(&mut engine, address, &[0x0f, 0x0b]);
    }
    upload(
        &mut engine,
        ENTRY,
        &[0xb8, 0x78, 0x56, 0x34, 0x12, 0x90, 0xc2, 8, 0],
    );
    word(&mut engine, esp, 0x2000);
    word(&mut engine, esp.wrapping_add(4), 0xfedc_ba98);
    word(&mut engine, esp.wrapping_add(8), 0x7654_3210);
    assert_eq!(compile(&mut engine), 1);
    engine
}

fn inject_stop(engine: &mut EngineInstance, state: State32, reason: ExitReason) {
    // native stop injection is a lifecycle input; actual callback execution has a separate oracle.
    encode_state(&state, &mut engine.arena_mut().unwrap()[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit { retired: 3, reason },
        &mut engine.arena_mut().unwrap()[56..96],
    )
    .unwrap();
}

fn parked(esp: u32) -> (EngineInstance, State32, u32) {
    let mut engine = fixture(esp);
    let state = original(esp);
    inject_stop(&mut engine, state, ExitReason::Gate { id: 17 });
    engine.arena_mut().unwrap()[100..140].fill(0x5a);
    let outer = engine
        .capture_call(KEY, 1, CallingConvention32::Stdcall, 2)
        .unwrap();
    (engine, state, outer.token)
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
    assert_eq!(operation(engine).err(), Some(expected));
    assert_eq!(engine.arena(), arena);
    assert_eq!(
        engine.artifact_bytes().map(|value| value.to_vec()),
        artifact
    );
    assert_eq!(engine.generation(), generation);
}

fn return_stop(engine: &mut EngineInstance, esp: u32, result: u32) {
    let mut state = decode_state(&engine.arena()[..56]).unwrap();
    state.eip = RETURN;
    state.registers[4] = esp;
    state.registers[0] = result;
    state.eflags = 2;
    inject_stop(engine, state, ExitReason::Gate { id: 18 });
}

fn wire(fields: [u32; 12]) -> [u8; 64] {
    let mut output = [0; 64];
    output[..16].copy_from_slice(&[0x52, 0x33, 0x43, 0x42, 1, 0, 1, 0, 64, 0, 0, 0, 0, 0, 0, 0]);
    for (index, value) in fields.into_iter().enumerate() {
        output[16 + index * 4..20 + index * 4].copy_from_slice(&value.to_le_bytes());
    }
    output
}

fn inner_stop(engine: &mut EngineInstance) {
    word(engine, 0x7ff0, ENTRY + 5);
    word(engine, 0x7ff4, 0x0123_4567);
    let mut state = decode_state(&engine.arena()[..56]).unwrap();
    state.eip = INNER;
    state.registers[4] = 0x7ff0;
    state.eflags = 2;
    inject_stop(engine, state, ExitReason::Gate { id: 19 });
}

#[test]
fn begin_publishes_callback_and_privately_suspends_outer() {
    let (mut engine, mut expected_state, outer) = parked(0x8010);
    let before = engine.arena().to_vec();
    let code = engine
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(OUTER), 2)
        .unwrap();
    let touched = engine
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(0x8000), 4)
        .unwrap();
    let unrelated = engine
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(0x7000), 4)
        .unwrap();
    let record = engine
        .begin_callback(
            KEY,
            1,
            outer,
            ENTRY,
            RETURN,
            18,
            &[0xabcd_ef01, 0x7654_3210],
        )
        .unwrap();
    assert_eq!(
        (
            record.token,
            record.outer_token,
            record.phase,
            record.outcome
        ),
        (2, 1, 1, 0)
    );
    assert_eq!(
        (
            record.entry_pc,
            record.entry_esp,
            record.return_pc,
            record.return_id
        ),
        (ENTRY, 0x8004, RETURN, 18)
    );
    assert_eq!(
        (record.stack_words, record.result, record.generation),
        (2, 0, 1)
    );
    expected_state.eip = ENTRY;
    expected_state.registers[4] = 0x8004;
    assert_eq!(decode_state(&engine.arena()[..56]), Ok(expected_state));
    let mut expected = before;
    expected[32..36].copy_from_slice(&0x8004u32.to_le_bytes());
    expected[48..52].copy_from_slice(&ENTRY.to_le_bytes());
    expected[56..96].copy_from_slice(&[
        0x52, 0x33, 0x45, 0x58, 3, 0, 1, 0, 40, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ]);
    expected[140..156]
        .copy_from_slice(&[0x52, 0x33, 0x43, 0x42, 1, 0, 1, 0, 64, 0, 0, 0, 0, 0, 0, 0]);
    for (index, value) in [2u32, 1, 1, 0, ENTRY, 0x8004, RETURN, 18, 2, 0, 1, 0]
        .into_iter()
        .enumerate()
    {
        expected[156 + index * 4..160 + index * 4].copy_from_slice(&value.to_le_bytes());
    }
    assert_eq!(engine.arena(), expected);
    assert_eq!(
        bytes(&engine, 0x8000, 16),
        [
            0, 0, 0, 0, 0, 0x12, 0, 0, 1, 0xef, 0xcd, 0xab, 0x10, 0x32, 0x54, 0x76
        ]
    );
    assert_eq!(
        bytes(&engine, 0x8010, 12),
        [
            0, 0x20, 0, 0, 0x98, 0xba, 0xdc, 0xfe, 0x10, 0x32, 0x54, 0x76
        ]
    );
    assert!(engine.memory().unwrap().is_code_current(&code));
    assert!(!engine.memory().unwrap().is_code_current(&touched));
    assert!(engine.memory().unwrap().is_code_current(&unrelated));
    engine.guard(KEY, 1).unwrap();
}

#[test]
fn transfer_begin_accepts_zero_and_sixteen_arguments_without_using_output_as_authority() {
    for count in [0, 16u32] {
        let (mut engine, mut expected, outer) = parked(0x8010);
        engine.arena_mut().unwrap()[140..].fill(0xff);
        for index in 0..count {
            engine.arena_mut().unwrap()[140 + index as usize * 4..144 + index as usize * 4]
                .copy_from_slice(&(0xa0b0_c000 + index).to_le_bytes());
        }
        let before = engine.arena().to_vec();
        let record = engine
            .begin_callback_from_transfer(KEY, 1, outer, ENTRY, RETURN, 18, count)
            .unwrap();
        let start = if count == 0 { 0x800c } else { 0x7fcc };
        assert_eq!(
            (record.token, record.stack_words, record.entry_esp),
            (2, count, start)
        );
        assert_eq!(bytes(&engine, start, 4), [0, 0x12, 0, 0]);
        for index in 0..count {
            assert_eq!(
                bytes(&engine, start + 4 + index * 4, 4),
                [index as u8, 0xc0, 0xb0, 0xa0]
            );
        }
        assert_eq!(&engine.arena()[96..140], &before[96..140]);
        assert_eq!(&engine.arena()[204..], &before[204..]);
        expected.eip = ENTRY;
        expected.registers[4] = start;
        assert_eq!(decode_state(&engine.arena()[..56]), Ok(expected));
        assert_eq!(
            &engine.arena()[140..204],
            &wire([2, 1, 1, 0, ENTRY, start, RETURN, 18, count, 0, 1, 0])
        );
    }
}

#[test]
fn begin_identity_count_and_token_failures_are_ordered_and_do_not_reserve_tokens() {
    let (mut engine, _, outer) = parked(0x8010);
    let frame = bytes(&engine, 0x7fcc, 68);
    for (key, generation) in [(KEY ^ 1, 1), (KEY, 0), (KEY, 2)] {
        unchanged(&mut engine, HostError::InvalidArtifact, |engine| {
            engine.begin_callback_from_transfer(key, generation, 0, 0, 0, 0, u32::MAX)
        });
    }
    unchanged(&mut engine, error(CallError::InvalidRequest), |engine| {
        engine.begin_callback(KEY, 1, 0, 0, 0, 0, &[0; 17])
    });
    for count in [17, u32::MAX] {
        unchanged(&mut engine, error(CallError::InvalidRequest), |engine| {
            engine.begin_callback_from_transfer(KEY, 1, 0, 0, 0, 0, count)
        });
    }
    for token in [0, outer + 1] {
        unchanged(&mut engine, error(CallError::InvalidToken), |engine| {
            engine.begin_callback(KEY, 1, token, ENTRY, RETURN, 18, &[])
        });
    }
    assert_eq!(bytes(&engine, 0x7fcc, 68), frame);
    assert_eq!(
        engine
            .begin_callback(KEY, 1, outer, ENTRY, RETURN, 18, &[])
            .unwrap()
            .token,
        2
    );
}

#[test]
fn saved_cpu_records_precede_cancel_and_cancel_precedes_entry_or_stack_checks() {
    let (mut engine, _, outer) = parked(0x8010);
    let saved = engine.arena()[..96].to_vec();
    engine.arena_mut().unwrap()[96..100].copy_from_slice(&1u32.to_le_bytes());
    for offset in [0, 16, 56, 76] {
        engine.arena_mut().unwrap()[offset] ^= 1;
        unchanged(&mut engine, error(CallError::StateChanged), |engine| {
            engine.begin_callback(KEY, 1, outer, ENTRY, RETURN, 18, &[])
        });
        engine.arena_mut().unwrap()[..96].copy_from_slice(&saved);
    }
    engine.unmap(0x7000, 2).unwrap();
    unchanged(&mut engine, error(CallError::Cancelled), |engine| {
        engine.begin_callback(KEY, 1, outer, u32::MAX, 0, 0, &[1; 16])
    });
    engine.map(0x7000, 2, 7).unwrap();
    engine.arena_mut().unwrap()[96..100].fill(0);
    assert_eq!(
        engine
            .begin_callback(KEY, 1, outer, ENTRY, RETURN, 18, &[])
            .unwrap()
            .token,
        2
    );
}

#[test]
fn entry_must_be_an_ordinary_instruction_boundary_and_return_a_distinct_bound_gate() {
    let (mut engine, _, outer) = parked(0x8010);
    for entry in [ENTRY + 1, ENTRY + 7, ENTRY + 9, OUTER, RETURN, INNER, 0] {
        unchanged(&mut engine, error(CallError::InvalidRequest), |engine| {
            engine.begin_callback(KEY, 1, outer, entry, RETURN, 18, &[])
        });
    }
    for (pc, id) in [
        (OUTER, 17),
        (RETURN, 19),
        (INNER, 18),
        (ENTRY, 18),
        (0x1400, 20),
    ] {
        unchanged(&mut engine, error(CallError::InvalidRequest), |engine| {
            engine.begin_callback(KEY, 1, outer, ENTRY, pc, id, &[])
        });
    }
    for entry in [ENTRY, ENTRY + 5, ENTRY + 6] {
        let record = engine
            .begin_callback(KEY, 1, outer, entry, RETURN, 18, &[])
            .unwrap();
        assert_eq!(record.entry_pc, entry);
        engine.abort_callback(KEY, record.token).unwrap();
    }
}

#[test]
fn explicit_frame_words_wrap_from_the_final_word_to_zero() {
    let (mut engine, mut expected, outer) = parked(4);
    engine.map(0xffff_f000, 1, 7).unwrap();
    let record = engine
        .begin_callback(KEY, 1, outer, ENTRY, RETURN, 18, &[0x4433_2211])
        .unwrap();
    assert_eq!(record.entry_esp, 0xffff_fffc);
    assert_eq!(bytes(&engine, 0xffff_fffc, 4), [0, 0x12, 0, 0]);
    assert_eq!(bytes(&engine, 0, 4), [0x11, 0x22, 0x33, 0x44]);
    assert_eq!(bytes(&engine, 4, 4), [0, 0x20, 0, 0]);
    expected.eip = ENTRY;
    expected.registers[4] = 0xffff_fffc;
    assert_eq!(decode_state(&engine.arena()[..56]), Ok(expected));
    return_stop(&mut engine, 4, 0x5566_7788);
    engine.finish_callback(KEY, 1, record.token).unwrap();
    engine.complete_call(KEY, 1, outer, 99).unwrap();
    assert_eq!(
        decode_state(&engine.arena()[..56]).unwrap().registers[4],
        16
    );
}

#[test]
fn last_argument_cross_page_and_width_faults_preserve_prefixes_stamps_and_outer_authority() {
    for kind in 0..5 {
        let esp = match kind {
            0 | 1 => 0x8004,
            2 => 0x8002,
            3 => 3,
            _ => 2,
        };
        let (mut engine, state, outer) = parked(esp);
        let (count, at, reason, prefix_address, prefix_length) = match kind {
            0 => {
                engine.protect(0x8000, 1, 5).unwrap();
                (16, 0x8000, FaultReason::Permission, 0x7fc0, 64)
            }
            1 => {
                engine.unmap(0x8000, 1).unwrap();
                (16, 0x8000, FaultReason::Unmapped, 0x7fc0, 64)
            }
            2 => {
                engine.protect(0x8000, 1, 5).unwrap();
                (0, 0x8000, FaultReason::Permission, 0x7ffc, 4)
            }
            3 => {
                engine.map(0xffff_f000, 1, 7).unwrap();
                (1, u32::MAX, FaultReason::AddressOverflow, 0xffff_fffb, 5)
            }
            _ => (0, 0xffff_fffe, FaultReason::AddressOverflow, 0, 16),
        };
        let prefix = bytes(&engine, prefix_address, prefix_length);
        let snapshot = engine
            .memory()
            .unwrap()
            .snapshot_code(GuestAddress(prefix_address), prefix_length)
            .unwrap();
        let mappings = engine.memory().unwrap().mapped_pages();
        unchanged(
            &mut engine,
            error(CallError::Memory(MemoryError::Fault(MemoryFault {
                address: GuestAddress(at),
                access: Access::Write,
                reason,
            }))),
            |engine| {
                engine.begin_callback(
                    KEY,
                    1,
                    outer,
                    ENTRY,
                    RETURN,
                    18,
                    &[0xaabb_ccdd; 16][..count],
                )
            },
        );
        assert_eq!(bytes(&engine, prefix_address, prefix_length), prefix);
        assert!(engine.memory().unwrap().is_code_current(&snapshot));
        assert_eq!(engine.memory().unwrap().mapped_pages(), mappings);
        assert_eq!(engine.guard(KEY, 1), Err(error(CallError::Busy)));
        engine.complete_call(KEY, 1, outer, 7).unwrap();
        if kind == 1 {
            engine.map(0x8000, 1, 7).unwrap();
            word(&mut engine, esp, 0x2000);
        }
        inject_stop(&mut engine, state, ExitReason::Gate { id: 17 });
        assert_eq!(
            engine
                .capture_call(KEY, 1, CallingConvention32::Cdecl, 0)
                .unwrap()
                .token,
            2
        );
    }
}

#[test]
fn outer_tokens_stay_busy_even_if_trusted_host_restores_the_saved_cpu_bytes() {
    let (mut engine, _, outer) = parked(0x8010);
    let saved = engine.arena()[..96].to_vec();
    let record = engine
        .begin_callback(KEY, 1, outer, ENTRY, RETURN, 18, &[])
        .unwrap();
    let started = engine.arena()[..96].to_vec();
    engine.arena_mut().unwrap()[..96].copy_from_slice(&saved);
    unchanged(&mut engine, error(CallError::Busy), |engine| {
        engine.complete_call(KEY, 1, outer, 7)
    });
    unchanged(&mut engine, error(CallError::Busy), |engine| {
        engine.abandon_call(KEY, outer)
    });
    unchanged(&mut engine, error(CallError::Busy), |engine| {
        engine.compile_with_gates(0, u32::MAX)
    });
    unchanged(&mut engine, error(CallError::Busy), |engine| {
        engine.begin_callback_from_transfer(KEY, 1, 0, 0, 0, 0, u32::MAX)
    });
    unchanged(&mut engine, HostError::InvalidArtifact, |engine| {
        engine.begin_callback(KEY ^ 1, 1, outer, ENTRY, RETURN, 18, &[])
    });
    engine.arena_mut().unwrap()[..96].copy_from_slice(&started);
    engine.guard(KEY, 1).unwrap();
    engine.abort_callback(KEY, record.token).unwrap();
    assert_eq!(&engine.arena()[..96], saved);
}

#[test]
fn ordinary_inner_calls_park_complete_abandon_and_retry_without_losing_callback_authority() {
    let (mut engine, _, outer) = parked(0x8010);
    let callback = engine
        .begin_callback(KEY, 1, outer, ENTRY, RETURN, 18, &[1, 2])
        .unwrap();
    inner_stop(&mut engine);
    let inner = engine
        .capture_call(KEY, 1, CallingConvention32::Cdecl, 1)
        .unwrap();
    assert_eq!(inner.token, 3);
    assert_eq!(inner.arguments[0], 0x0123_4567);
    assert_eq!(engine.guard(KEY, 1), Err(error(CallError::Busy)));
    unchanged(&mut engine, error(CallError::InvalidToken), |engine| {
        engine.finish_callback(KEY, 1, callback.token + 1)
    });
    unchanged(&mut engine, error(CallError::Busy), |engine| {
        engine.finish_callback(KEY, 1, callback.token)
    });
    unchanged(&mut engine, error(CallError::Busy), |engine| {
        engine.begin_callback(KEY, 1, inner.token, ENTRY, RETURN, 18, &[0; 17])
    });
    unchanged(&mut engine, error(CallError::Busy), |engine| {
        engine.complete_call(KEY, 1, outer, 7)
    });
    engine
        .complete_call(KEY, 1, inner.token, 0xa1b2_c3d4)
        .unwrap();
    let resumed = decode_state(&engine.arena()[..56]).unwrap();
    assert_eq!(
        (resumed.eip, resumed.registers[4], resumed.registers[0]),
        (ENTRY + 5, 0x7ff4, 0xa1b2_c3d4)
    );
    engine.guard(KEY, 1).unwrap();
    inner_stop(&mut engine);
    let abandoned = engine
        .capture_call(KEY, 1, CallingConvention32::Cdecl, 1)
        .unwrap();
    assert_eq!(abandoned.token, 4);
    let saved = engine.arena().to_vec();
    engine.abandon_call(KEY, abandoned.token).unwrap();
    assert_eq!(engine.arena(), saved);
    let retry = engine
        .capture_call(KEY, 1, CallingConvention32::Cdecl, 1)
        .unwrap();
    assert_eq!(retry.token, 5);
    unchanged(&mut engine, error(CallError::InvalidToken), |engine| {
        engine.complete_call(KEY, 1, abandoned.token, 7)
    });
    engine.complete_call(KEY, 1, retry.token, 9).unwrap();
    return_stop(&mut engine, 0x8010, 10);
    assert_eq!(
        engine
            .finish_callback(KEY, 1, callback.token)
            .unwrap()
            .result,
        10
    );
}

#[test]
fn private_return_gate_remains_reserved_despite_transfer_tampering_or_bad_stack_address() {
    let (mut engine, _, outer) = parked(0x8010);
    let callback = engine
        .begin_callback(KEY, 1, outer, ENTRY, RETURN, 18, &[])
        .unwrap();
    engine.arena_mut().unwrap()[180..184].copy_from_slice(&INNER.to_le_bytes());
    engine.arena_mut().unwrap()[184..188].copy_from_slice(&19u32.to_le_bytes());
    return_stop(&mut engine, u32::MAX, 7);
    unchanged(&mut engine, error(CallError::InvalidStop), |engine| {
        engine.capture_call(KEY, 1, CallingConvention32::Cdecl, 0)
    });
    return_stop(&mut engine, 0x8010, 7);
    engine.finish_callback(KEY, 1, callback.token).unwrap();
    assert_eq!(
        engine.capture_call(KEY, 1, CallingConvention32::Cdecl, 0),
        Err(error(CallError::Busy))
    );
}

#[test]
fn finish_requires_canonical_exact_gate_and_cleaned_stack_before_cancel() {
    for change in 0..7 {
        let (mut engine, _, outer) = parked(0x8010);
        let callback = engine
            .begin_callback(KEY, 1, outer, ENTRY, RETURN, 18, &[1, 2])
            .unwrap();
        return_stop(&mut engine, 0x8010, 0xaabb_ccdd);
        let good = engine.arena()[..96].to_vec();
        if change == 0 {
            for (key, generation) in [(KEY ^ 1, 1), (KEY, 0), (KEY, 2)] {
                unchanged(&mut engine, HostError::InvalidArtifact, |engine| {
                    engine.finish_callback(key, generation, callback.token)
                });
            }
            for token in [0, callback.token + 1] {
                unchanged(&mut engine, error(CallError::InvalidToken), |engine| {
                    engine.finish_callback(KEY, 1, token)
                });
            }
        }
        match change {
            0 => engine.arena_mut().unwrap()[0] = 0,
            1 => engine.arena_mut().unwrap()[56] = 0,
            2 => engine.arena_mut().unwrap()[48..52].copy_from_slice(&INNER.to_le_bytes()),
            3 => engine.arena_mut().unwrap()[80..84].copy_from_slice(&19u32.to_le_bytes()),
            4 => engine.arena_mut().unwrap()[32..36].copy_from_slice(&0x800cu32.to_le_bytes()),
            5 => {
                engine.arena_mut().unwrap()[72..76].copy_from_slice(&3u32.to_le_bytes());
                engine.arena_mut().unwrap()[80..84].fill(0);
            }
            _ => {
                engine.arena_mut().unwrap()[48..52].copy_from_slice(&OUTER.to_le_bytes());
                engine.arena_mut().unwrap()[80..84].copy_from_slice(&17u32.to_le_bytes());
            }
        }
        engine.arena_mut().unwrap()[96..100].copy_from_slice(&1u32.to_le_bytes());
        let frame = bytes(&engine, 0x8004, 12);
        unchanged(&mut engine, error(CallError::InvalidStop), |engine| {
            engine.finish_callback(KEY, 1, callback.token)
        });
        assert_eq!(bytes(&engine, 0x8004, 12), frame);
        engine.arena_mut().unwrap()[..96].copy_from_slice(&good);
        unchanged(&mut engine, error(CallError::Cancelled), |engine| {
            engine.finish_callback(KEY, 1, callback.token)
        });
        engine.arena_mut().unwrap()[96..100].fill(0);
        assert_eq!(
            engine
                .finish_callback(KEY, 1, callback.token)
                .unwrap()
                .result,
            0xaabb_ccdd
        );
    }
}

#[test]
fn finish_captures_eax_and_restores_only_private_outer_records_without_rereading_ram() {
    let (mut engine, state, outer) = parked(0x8010);
    let saved = engine.arena()[..96].to_vec();
    let callback = engine
        .begin_callback(KEY, 1, outer, ENTRY, RETURN, 18, &[0x11, 0x22])
        .unwrap();
    word(&mut engine, 0x8004, 0xdead_beef);
    word(&mut engine, 0x8010, u32::MAX);
    engine.unmap(0x7000, 2).unwrap();
    engine.arena_mut().unwrap()[140..].fill(0xff);
    return_stop(&mut engine, 0x8010, 0x8765_4321);
    let before = engine.arena().to_vec();
    let record = engine.finish_callback(KEY, 1, callback.token).unwrap();
    assert_eq!(
        (
            record.phase,
            record.outcome,
            record.result,
            record.outer_token
        ),
        (2, 0, 0x8765_4321, outer)
    );
    let mut expected = before;
    expected[..96].copy_from_slice(&saved);
    expected[140..204].copy_from_slice(&wire([
        2,
        1,
        2,
        0,
        ENTRY,
        0x8004,
        RETURN,
        18,
        2,
        0x8765_4321,
        1,
        0,
    ]));
    assert_eq!(engine.arena(), expected);
    assert_eq!(decode_state(&engine.arena()[..56]), Ok(state));
    assert_eq!(engine.guard(KEY, 1), Err(error(CallError::Busy)));
    engine.complete_call(KEY, 1, outer, 99).unwrap();
    let mut completed = state;
    completed.registers[0] = 99;
    completed.registers[4] = 0x801c;
    completed.eip = 0x2000;
    assert_eq!(decode_state(&engine.arena()[..56]), Ok(completed));
    assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
}

#[test]
fn successive_callbacks_keep_outer_token_and_reject_consumed_callback_handles() {
    let (mut engine, _, outer) = parked(0x8010);
    let first = engine
        .begin_callback(KEY, 1, outer, ENTRY, RETURN, 18, &[])
        .unwrap();
    return_stop(&mut engine, 0x8010, 0);
    engine.finish_callback(KEY, 1, first.token).unwrap();
    unchanged(&mut engine, error(CallError::InvalidToken), |engine| {
        engine.finish_callback(KEY, 1, first.token)
    });
    unchanged(&mut engine, error(CallError::InvalidToken), |engine| {
        engine.abort_callback(KEY, first.token)
    });
    let second = engine
        .begin_callback(KEY, 1, outer, ENTRY + 5, RETURN, 18, &[1])
        .unwrap();
    assert_eq!((first.token, second.token, second.outer_token), (2, 3, 1));
    unchanged(&mut engine, error(CallError::InvalidToken), |engine| {
        engine.finish_callback(KEY, 1, first.token)
    });
    unchanged(&mut engine, error(CallError::InvalidToken), |engine| {
        engine.abort_callback(KEY, first.token)
    });
    return_stop(&mut engine, 0x8010, u32::MAX);
    assert_eq!(
        engine.finish_callback(KEY, 1, second.token).unwrap().result,
        u32::MAX
    );
    engine.complete_call(KEY, 1, outer, 7).unwrap();
    inject_stop(&mut engine, original(0x8010), ExitReason::Gate { id: 17 });
    assert_eq!(
        engine
            .capture_call(KEY, 1, CallingConvention32::Cdecl, 0)
            .unwrap()
            .token,
        4
    );
}

#[test]
fn same_byte_code_frame_is_committed_and_keeps_an_abortable_stale_continuation() {
    let mut engine = fixture(0x1080);
    for (address, value) in [(0x1074, RETURN), (0x1078, 0x11), (0x107c, 0x22)] {
        word(&mut engine, address, value);
    }
    assert_eq!(compile(&mut engine), 2);
    let state = original(0x1080);
    inject_stop(&mut engine, state, ExitReason::Gate { id: 17 });
    let outer = engine
        .capture_call(KEY, 2, CallingConvention32::Stdcall, 2)
        .unwrap();
    let saved = engine.arena()[..96].to_vec();
    let code = engine
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(OUTER), 2)
        .unwrap();
    let frame = bytes(&engine, 0x1074, 12);
    let callback = engine
        .begin_callback(KEY, 2, outer.token, ENTRY, RETURN, 18, &[0x11, 0x22])
        .unwrap();
    assert_eq!(
        (
            callback.token,
            callback.phase,
            callback.outcome,
            callback.generation
        ),
        (2, 1, 1, 2)
    );
    assert_eq!(bytes(&engine, 0x1074, 12), frame);
    assert!(!engine.memory().unwrap().is_code_current(&code));
    assert_eq!(
        &engine.arena()[140..204],
        &wire([2, 1, 1, 1, ENTRY, 0x1074, RETURN, 18, 2, 0, 2, 0])
    );
    assert_eq!(engine.guard(KEY, 2), Err(HostError::CodeInvalidated));
    unchanged(&mut engine, HostError::CodeInvalidated, |engine| {
        engine.finish_callback(KEY, 2, callback.token)
    });
    unchanged(&mut engine, HostError::CodeInvalidated, |engine| {
        engine.begin_callback(KEY, 2, outer.token, ENTRY, RETURN, 18, &[0; 17])
    });
    let before = engine.arena().to_vec();
    engine.abort_callback(KEY, callback.token).unwrap();
    let mut expected = before;
    expected[..96].copy_from_slice(&saved);
    assert_eq!(engine.arena(), expected);
    unchanged(&mut engine, HostError::CodeInvalidated, |engine| {
        engine.complete_call(KEY, 2, outer.token, 7)
    });
    unchanged(&mut engine, error(CallError::Busy), |engine| {
        engine.compile(0)
    });
    engine.abandon_call(KEY, outer.token).unwrap();
    assert_eq!(compile(&mut engine), 3);
    inject_stop(&mut engine, state, ExitReason::Gate { id: 17 });
    assert_eq!(
        engine
            .capture_call(KEY, 3, CallingConvention32::Cdecl, 0)
            .unwrap()
            .token,
        3
    );
}

#[test]
fn forced_abort_discards_inner_authority_and_restores_outer_even_with_stale_cancelled_corrupt_state()
 {
    let (mut engine, state, outer) = parked(0x8010);
    let saved = engine.arena()[..96].to_vec();
    let callback = engine
        .begin_callback(KEY, 1, outer, ENTRY, RETURN, 18, &[0x11, 0x22])
        .unwrap();
    inner_stop(&mut engine);
    let inner = engine
        .capture_call(KEY, 1, CallingConvention32::Cdecl, 1)
        .unwrap();
    engine.arena_mut().unwrap()[100..140].fill(0xa5);
    engine.arena_mut().unwrap()[140..].fill(0xff);
    engine.arena_mut().unwrap()[96..100].copy_from_slice(&1u32.to_le_bytes());
    engine.arena_mut().unwrap()[0] = 0;
    engine.arena_mut().unwrap()[56] = 0;
    upload(&mut engine, OUTER, &[0x0f, 0x0b]);
    let guest = bytes(&engine, 0x7ff0, 44);
    let before = engine.arena().to_vec();
    unchanged(&mut engine, HostError::InvalidArtifact, |engine| {
        engine.abort_callback(KEY ^ 1, callback.token)
    });
    unchanged(&mut engine, error(CallError::InvalidToken), |engine| {
        engine.abort_callback(KEY, callback.token + 1)
    });
    engine.abort_callback(KEY, callback.token).unwrap();
    let mut expected = before;
    expected[..96].copy_from_slice(&saved);
    assert_eq!(engine.arena(), expected);
    assert_eq!(bytes(&engine, 0x7ff0, 44), guest);
    unchanged(&mut engine, error(CallError::InvalidToken), |engine| {
        engine.abort_callback(KEY, callback.token)
    });
    unchanged(&mut engine, error(CallError::InvalidToken), |engine| {
        engine.abandon_call(KEY, inner.token)
    });
    unchanged(&mut engine, HostError::CodeInvalidated, |engine| {
        engine.complete_call(KEY, 1, outer, 7)
    });
    unchanged(&mut engine, error(CallError::Busy), |engine| {
        engine.compile(0)
    });
    engine.abandon_call(KEY, outer).unwrap();
    assert_eq!(compile(&mut engine), 2);
    engine.arena_mut().unwrap()[96..100].fill(0);
    inject_stop(&mut engine, state, ExitReason::Gate { id: 17 });
    assert_eq!(
        engine
            .capture_call(KEY, 2, CallingConvention32::Cdecl, 0)
            .unwrap()
            .token,
        4
    );
}

#[test]
fn close_revokes_callback_outer_and_inner_tokens_without_changing_the_arena() {
    let (mut engine, _, outer) = parked(0x8010);
    let callback = engine
        .begin_callback(KEY, 1, outer, ENTRY, RETURN, 18, &[])
        .unwrap();
    inner_stop(&mut engine);
    let inner = engine
        .capture_call(KEY, 1, CallingConvention32::Cdecl, 0)
        .unwrap();
    let before = engine.arena().to_vec();
    engine.close();
    assert_eq!(engine.arena(), before);
    unchanged(&mut engine, HostError::Closed, |engine| {
        engine.begin_callback_from_transfer(KEY, 1, outer, 0, 0, 0, u32::MAX)
    });
    unchanged(&mut engine, HostError::Closed, |engine| {
        engine.finish_callback(KEY, 1, callback.token)
    });
    unchanged(&mut engine, HostError::Closed, |engine| {
        engine.abort_callback(KEY, callback.token)
    });
    for token in [outer, inner.token] {
        unchanged(&mut engine, HostError::Closed, |engine| {
            engine.complete_call(KEY, 1, token, 7)
        });
        unchanged(&mut engine, HostError::Closed, |engine| {
            engine.abandon_call(KEY, token)
        });
    }
}
