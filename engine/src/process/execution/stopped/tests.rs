use crate::{
    abi::{
        arena::{CANCEL_OFFSET, EXIT_OFFSET, SIMD_OFFSET, TRANSFER_OFFSET},
        x86::{EXIT_SIZE, STATE_SIZE, encode_exit, encode_state_v2},
    },
    cpu::{ExecutionExit, ExitReason, x86::State32},
    process::{CallError, EngineInstance, HostError},
};

const KEY: u64 = 0x5354_4f50_454e_5452;
const ENTRY: u32 = 0x2000;

fn upload(engine: &mut EngineInstance, address: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
        .copy_from_slice(bytes);
    engine.upload(address, bytes.len() as u32).unwrap();
}

fn stopped(bytes: &[u8]) -> EngineInstance {
    let mut engine = EngineInstance::new(2, KEY).unwrap();
    for address in [0x1000, ENTRY] {
        engine.map(address, 1, 7).unwrap();
    }
    upload(&mut engine, 0x1000, &[0xe9, 0xfb, 0x0f, 0, 0]);
    upload(&mut engine, ENTRY, bytes);
    let transfer = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    transfer[..4].copy_from_slice(&0x1000_u32.to_le_bytes());
    transfer[4..].copy_from_slice(&5_u32.to_le_bytes());
    engine.compile(1).unwrap();
    set_stop(&mut engine, ENTRY, ExitReason::NeedCode);
    engine
}

fn set_stop(engine: &mut EngineInstance, eip: u32, reason: ExitReason) {
    let arena = engine.arena_mut().unwrap();
    encode_state_v2(
        &State32 {
            registers: [0x8182_8384; 8],
            eip,
            eflags: 0x244cd7,
        },
        &mut arena[..STATE_SIZE],
    )
    .unwrap();
    arena[12..16].copy_from_slice(&1_u32.to_le_bytes());
    encode_exit(
        &ExecutionExit { retired: 1, reason },
        &mut arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
}

fn rejected_without_publication(
    engine: &mut EngineInstance,
    key: u64,
    generation: u32,
    expected: HostError,
) {
    let before = engine.arena().to_vec();
    let old_generation = engine.generation();
    let old_artifact = engine.artifact_bytes().map(<[u8]>::to_vec);
    assert_eq!(engine.compile_stopped_entry(key, generation), Err(expected));
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.generation(), old_generation);
    assert_eq!(engine.artifact_bytes().map(<[u8]>::to_vec), old_artifact);
}

#[test]
fn stopped_entry_publishes_only_current_entry_and_preserves_cpu_records() {
    for bytes in [&[0xf0, 0x90][..], &[0xf4][..], &[0x90, 0xeb, 0][..]] {
        let mut engine = stopped(bytes);
        let before = engine.arena().to_vec();
        assert_eq!(engine.compile_stopped_entry(KEY, 1), Ok(2));
        assert_eq!(engine.generation(), 2);
        assert_eq!(engine.arena(), before);
        assert!(
            engine
                .artifact
                .as_ref()
                .unwrap()
                .contains_instruction(ENTRY)
        );
        assert!(
            !engine
                .artifact
                .as_ref()
                .unwrap()
                .contains_instruction(0x1000)
        );
        assert_eq!(engine.guard(KEY, 1), Err(HostError::InvalidArtifact));
        engine.guard(KEY, 2).unwrap();
    }
}

#[test]
fn stopped_entry_rejects_wrong_owner_generation_stop_and_existing_entry() {
    let mut engine = stopped(&[0xf0, 0x90]);
    for (key, generation) in [(KEY + 1, 1), (KEY, 0), (KEY, 2)] {
        rejected_without_publication(&mut engine, key, generation, HostError::InvalidArtifact);
    }
    for reason in [ExitReason::Budget, ExitReason::Cancelled] {
        set_stop(&mut engine, ENTRY, reason);
        rejected_without_publication(&mut engine, KEY, 1, HostError::Call(CallError::InvalidStop));
    }
    set_stop(&mut engine, 0x1000, ExitReason::NeedCode);
    rejected_without_publication(&mut engine, KEY, 1, HostError::Call(CallError::InvalidStop));
}

#[test]
fn stopped_entry_refuses_cancelled_or_malformed_cpu_context_atomically() {
    let mut engine = stopped(&[0xf0, 0x90]);
    engine.arena_mut().unwrap()[CANCEL_OFFSET] = 1;
    rejected_without_publication(&mut engine, KEY, 1, HostError::Call(CallError::Cancelled));
    engine.arena_mut().unwrap()[CANCEL_OFFSET] = 0;
    for offset in [12, SIMD_OFFSET + 18] {
        let old = engine.arena()[offset];
        engine.arena_mut().unwrap()[offset] = 0xff;
        rejected_without_publication(&mut engine, KEY, 1, HostError::Call(CallError::InvalidStop));
        engine.arena_mut().unwrap()[offset] = old;
    }
}

#[test]
fn stopped_entry_keeps_strict_admission_and_failed_preparation_atomic() {
    let mut engine = stopped(&[0xf0, 0x90]);
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 4]
        .copy_from_slice(&ENTRY.to_le_bytes());
    let before = engine.arena().to_vec();
    assert!(matches!(
        engine.compile_entries(1, 0),
        Err(HostError::Compile(_))
    ));
    assert_eq!(engine.generation(), 1);
    assert_eq!(engine.arena(), before);

    // avx is valid but unsupported; runtime entry must retain that distinction.
    upload(&mut engine, ENTRY, &[0xc5, 0xf8, 0x58, 0xc1]);
    let before = engine.arena().to_vec();
    let old = engine.artifact_bytes().unwrap().to_vec();
    assert!(matches!(
        engine.compile_stopped_entry(KEY, 1),
        Err(HostError::Compile(_))
    ));
    assert_eq!(engine.generation(), 1);
    assert_eq!(engine.artifact_bytes().unwrap(), old);
    assert_eq!(engine.arena(), before);
}

#[test]
fn stopped_entry_requires_live_prior_snapshot_and_available_generation() {
    let mut engine = stopped(&[0xf0, 0x90]);
    upload(&mut engine, 0x1000, &[0x90]);
    rejected_without_publication(&mut engine, KEY, 1, HostError::CodeInvalidated);

    let mut engine = stopped(&[0xf0, 0x90]);
    engine.generation = u32::MAX;
    rejected_without_publication(&mut engine, KEY, u32::MAX, HostError::GenerationExhausted);
}

#[test]
fn stopped_entry_cannot_replace_code_owned_by_a_pending_call() {
    use crate::{
        abi::x86::decode_state,
        process::calls::{
            call::{PendingCall, PendingOwner},
            context::ExtendedCpuState,
        },
        windows::{CallFrame32, CallingConvention32},
    };
    let mut engine = stopped(&[0xf0, 0x90]);
    let mut state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
    state.registers[4] = ENTRY + 16;
    let frame = CallFrame32::capture(
        engine.memory().unwrap(),
        state,
        CallingConvention32::Cdecl,
        0,
    )
    .unwrap();
    engine.pending_call = Some(PendingCall {
        token: 1,
        owner: PendingOwner::Replacement(1),
        frame,
        state: engine.arena()[..STATE_SIZE].try_into().unwrap(),
        exit: engine.arena()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE]
            .try_into()
            .unwrap(),
        extended: ExtendedCpuState::capture(engine.arena()).unwrap(),
    });
    rejected_without_publication(&mut engine, KEY, 1, HostError::Call(CallError::Busy));
    assert_eq!(engine.pending_call.as_ref().unwrap().token, 1);
}
