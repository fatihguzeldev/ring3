use super::*;
use crate::{
    abi::{
        arena::{STATE_OFFSET, TRANSFER_OFFSET},
        x86::{STATE_SIZE, encode_exit_v3, encode_state},
    },
    cpu::x86::{Register32, State32},
    process::call::PendingOwner,
    windows::CallingConvention32,
};

const KEY: u64 = 0xa113_2233_4455_6677;
const CODE: u32 = 0x1000;
const KEEP: u32 = 0x2000;
const STACK: u32 = 0x8000;

fn words(engine: &mut EngineInstance, values: &[u32]) {
    for (index, value) in values.iter().enumerate() {
        let at = TRANSFER_OFFSET + index * 4;
        engine.arena_mut().unwrap()[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
}

fn fixture(resident: bool) -> (EngineInstance, PendingOwner, u64) {
    let mut engine = EngineInstance::new(5, KEY).unwrap();
    for (address, bytes) in [
        (CODE, &[0x0f, 0x0b][..]),
        (KEEP, &[0x90][..]),
        (STACK, &[0][..]),
    ] {
        engine.map(address, 1, 7).unwrap();
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(address), bytes)
            .unwrap();
    }
    words(&mut engine, &[KEEP, 1]);
    let keep = engine.compile_resident(1).unwrap().get();
    words(
        &mut engine,
        &[CODE, 2, CODE, WindowsApi32::VirtualAlloc.id()],
    );
    engine.compile_with_gates(1, 1).unwrap();
    let caller = engine.compile_resident_with_gates(1, 1).unwrap().get();
    for (index, value) in [0x9000_u32, 0, 4097, 0x3000, 4].into_iter().enumerate() {
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(STACK + index as u32 * 4), &value.to_le_bytes())
            .unwrap();
    }
    let mut state = State32::default();
    state.eip = CODE;
    state.eflags = 0xcd7;
    state.registers = [
        0x1234_5678,
        0x2345_6789,
        0x3456_789a,
        0x4567_89ab,
        STACK,
        0x5678_9abc,
        0x6789_abcd,
        0x789a_bcde,
    ];
    encode_state(
        &state,
        &mut engine.arena_mut().unwrap()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE],
    )
    .unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate {
                id: WindowsApi32::VirtualAlloc.id(),
            },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    let owner = if resident {
        PendingOwner::Resident(caller)
    } else {
        PendingOwner::Replacement(engine.generation())
    };
    match owner {
        PendingOwner::Replacement(generation) => {
            engine
                .capture_call(KEY, generation, CallingConvention32::Stdcall, 4)
                .unwrap();
        }
        PendingOwner::Resident(id) => {
            engine
                .capture_resident_call(KEY, id, CallingConvention32::Stdcall, 4)
                .unwrap();
        }
        PendingOwner::ResidentCallback { .. } => unreachable!(),
    }
    (engine, owner, keep)
}

fn complete(engine: &mut EngineInstance, owner: PendingOwner, token: u32) -> Result<(), HostError> {
    match owner {
        PendingOwner::Replacement(generation) => {
            engine.complete_windows_call(KEY, generation, token)
        }
        PendingOwner::Resident(id) => engine.complete_resident_windows_call(KEY, id, token),
        PendingOwner::ResidentCallback { .. } => unreachable!(),
    }
}

fn pending(
    engine: &EngineInstance,
) -> (
    u32,
    PendingOwner,
    CallFrame32,
    [u8; STATE_SIZE],
    [u8; EXIT_SIZE],
) {
    let call = engine.pending_call.as_ref().unwrap();
    (call.token, call.owner, call.frame, call.state, call.exit)
}

fn assert_unchanged(
    engine: &EngineInstance,
    arena: &[u8],
    call: &(
        u32,
        PendingOwner,
        CallFrame32,
        [u8; STATE_SIZE],
        [u8; EXIT_SIZE],
    ),
    identity: u64,
    keep: u64,
) {
    assert_eq!(engine.arena(), arena);
    assert_eq!(&pending(engine), call);
    let memory = engine.memory.as_ref().unwrap();
    assert_eq!(memory.identity(), identity);
    assert_eq!(memory.mapped_pages(), 3);
    assert_eq!(
        memory.resolve(GuestAddress(0x1000_0000), crate::memory::Access::Read),
        Err(MemoryError::Fault(crate::memory::MemoryFault {
            address: GuestAddress(0x1000_0000),
            access: crate::memory::Access::Read,
            reason: crate::memory::FaultReason::Unmapped
        }))
    );
    let error_frame =
        CallFrame32::capture(memory, *call.2.state(), CallingConvention32::Stdcall, 0).unwrap();
    let (last_error, _) = engine
        .windows_thread
        .prepare(
            WindowsApi32::GetLastError,
            &error_frame,
            ProcessContext32::default(),
        )
        .unwrap();
    assert!(matches!(last_error, WindowsOutcome32::Return(0)));
    engine.guard_artifact(KEY, engine.generation()).unwrap();
    engine.guard_resident_unit(KEY, keep).unwrap();
    assert_eq!(
        engine.guard(KEY, engine.generation()),
        Err(HostError::Call(super::super::CallError::Busy))
    );
    assert_eq!(
        engine.guard_resident(KEY, keep),
        Err(HostError::Call(super::super::CallError::Busy))
    );
    if let PendingOwner::Resident(caller) = call.1 {
        engine.guard_resident_unit(KEY, caller).unwrap();
    }
}

#[test]
fn exhausted_mapping_version_preserves_pending_call_arena_and_all_published_owners() {
    for resident in [false, true] {
        let (mut engine, owner, keep) = fixture(resident);
        engine.memory.as_mut().unwrap().exhaust_versions_for_test();
        let arena = engine.arena().to_vec();
        let call = pending(&engine);
        let identity = engine.memory.as_ref().unwrap().identity();
        assert_eq!(
            complete(&mut engine, owner, call.0),
            Err(HostError::Memory(MemoryError::VersionExhausted))
        );
        assert_unchanged(&engine, &arena, &call, identity, keep);
        assert_eq!(
            complete(&mut engine, owner, call.0),
            Err(HostError::Memory(MemoryError::VersionExhausted))
        );
        assert_unchanged(&engine, &arena, &call, identity, keep);
    }
}

#[test]
fn invalid_internal_return_encoding_fails_before_mapping_or_consuming_the_call() {
    for resident in [false, true] {
        let (mut engine, owner, keep) = fixture(resident);
        let mut invalid = *engine.pending_call.as_ref().unwrap().frame.state();
        invalid.eflags = 0;
        let frame = CallFrame32::capture(
            engine.memory.as_ref().unwrap(),
            invalid,
            CallingConvention32::Stdcall,
            4,
        )
        .unwrap();
        engine.pending_call.as_mut().unwrap().frame = frame;
        let arena = engine.arena().to_vec();
        let call = pending(&engine);
        let identity = engine.memory.as_ref().unwrap().identity();
        assert_eq!(
            complete(&mut engine, owner, call.0),
            Err(HostError::Infrastructure)
        );
        assert_unchanged(&engine, &arena, &call, identity, keep);
        let mut valid = invalid;
        valid.eflags = 0xcd7;
        let frame = CallFrame32::capture(
            engine.memory.as_ref().unwrap(),
            valid,
            CallingConvention32::Stdcall,
            4,
        )
        .unwrap();
        engine.pending_call.as_mut().unwrap().frame = frame;
        complete(&mut engine, owner, call.0).unwrap();
        assert!(engine.pending_call.is_none());
        assert_eq!(engine.memory.as_ref().unwrap().mapped_pages(), 5);
        let state =
            crate::abi::x86::decode_state(&engine.arena()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE])
                .unwrap();
        assert_eq!(state.registers[Register32::Eax.index()], 0x1000_0000);
        assert_eq!(state.eflags, 0xcd7);
        engine.guard(KEY, engine.generation()).unwrap();
        engine.guard_resident(KEY, keep).unwrap();
    }
}
