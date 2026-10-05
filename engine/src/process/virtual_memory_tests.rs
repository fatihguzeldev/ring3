use crate::{
    abi::{
        arena::{EXIT_OFFSET, STATE_OFFSET, TRANSFER_OFFSET},
        x86::{EXIT_SIZE, STATE_SIZE, decode_exit, decode_state, encode_exit_v3, encode_state},
    },
    cpu::{ExecutionExit, ExitReason, x86::State32},
    memory::{CodeSnapshot, GuestAddress, MemoryError, PageRange},
    process::{EngineInstance, HostError, call::PendingOwner},
    windows::{CallFrame32, CallingConvention32, ProcessContext32, WindowsApi32, WindowsOutcome32},
};

const KEY: u64 = 0xa381_1234_5678_abcd;
const CODE: u32 = 0x1000;
const KEEP: u32 = 0x2000;
const STACK: u32 = 0x8000;
const BASE: u32 = 0x1000_0000;
const RETURN: u32 = CODE + 64;
const PAGES: [u32; 5] = [CODE, KEEP, STACK, BASE, BASE + 4096];

struct Fixture {
    engine: EngineInstance,
    caller: u64,
    keeper: u64,
}

fn words(engine: &mut EngineInstance, values: &[u32]) {
    for (index, value) in values.iter().enumerate() {
        let offset = TRANSFER_OFFSET + index * 4;
        engine.arena_mut().unwrap()[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
}

fn fixture() -> Fixture {
    let mut engine = EngineInstance::new(8, KEY).unwrap();
    for address in [CODE, KEEP, STACK] {
        engine.map(address, 1, 7).unwrap();
    }
    for address in [CODE, CODE + 16, CODE + 32] {
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(address), &[0x0f, 0x0b])
            .unwrap();
    }
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(KEEP), &[0x90])
        .unwrap();
    words(
        &mut engine,
        &[
            CODE,
            2,
            CODE + 16,
            2,
            CODE + 32,
            2,
            CODE,
            WindowsApi32::VirtualAlloc.id(),
            CODE + 16,
            WindowsApi32::VirtualFree.id(),
            CODE + 32,
            WindowsApi32::ExitProcess.id(),
        ],
    );
    engine.compile_with_gates(3, 3).unwrap();
    let caller = engine.compile_resident_with_gates(3, 3).unwrap().get();
    words(&mut engine, &[KEEP, 1]);
    let keeper = engine.compile_resident(1).unwrap().get();
    engine.windows_thread = engine.windows_thread.allocation_failed();
    Fixture {
        engine,
        caller,
        keeper,
    }
}

fn capture(
    engine: &mut EngineInstance,
    pc: u32,
    esp: u32,
    api: WindowsApi32,
    arguments: &[u32],
) -> (u32, State32) {
    for (index, value) in [RETURN].iter().chain(arguments).enumerate() {
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(esp + index as u32 * 4), &value.to_le_bytes())
            .unwrap();
    }
    // typed native Gate input; no generated guest execution is claimed.
    let state = State32 {
        registers: [
            0x1234_5678,
            0x2345_6789,
            0x3456_789a,
            0x4567_89ab,
            esp,
            0x5678_9abc,
            0x6789_abcd,
            0x789a_bcde,
        ],
        eip: pc,
        eflags: 0xcd7,
    };
    let arena = engine.arena_mut().unwrap();
    encode_state(&state, &mut arena[STATE_OFFSET..STATE_OFFSET + STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 7,
            reason: ExitReason::Gate { id: api.id() },
        },
        &mut arena[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    let generation = engine.generation();
    let call = engine
        .capture_call(
            KEY,
            generation,
            CallingConvention32::Stdcall,
            arguments.len() as u32,
        )
        .unwrap();
    (call.token, state)
}

fn complete(engine: &mut EngineInstance, token: u32) -> Result<(), HostError> {
    engine.complete_windows_call(KEY, engine.generation(), token)
}

fn allocate(engine: &mut EngineInstance, size: u32) -> u32 {
    let (token, _) = capture(
        engine,
        CODE,
        STACK,
        WindowsApi32::VirtualAlloc,
        &[0, size, 0x3000, 4],
    );
    complete(engine, token).unwrap();
    decode_state(&engine.arena()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE])
        .unwrap()
        .registers[0]
}

fn release(engine: &mut EngineInstance, address: u32) -> u32 {
    let (token, _) = capture(
        engine,
        CODE + 16,
        STACK,
        WindowsApi32::VirtualFree,
        &[address, 0, 0x8000],
    );
    complete(engine, token).unwrap();
    decode_state(&engine.arena()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE])
        .unwrap()
        .registers[0]
}

fn last_error(engine: &EngineInstance) -> u32 {
    let mut state = State32::default();
    state.registers[4] = STACK;
    let frame = CallFrame32::capture(
        engine.memory.as_ref().unwrap(),
        state,
        CallingConvention32::Stdcall,
        0,
    )
    .unwrap();
    let (outcome, _) = engine
        .windows_thread
        .prepare(
            WindowsApi32::GetLastError,
            &frame,
            ProcessContext32::default(),
        )
        .unwrap();
    let WindowsOutcome32::Return(value) = outcome else {
        panic!("expected last-error return")
    };
    value
}

type Pending = (
    u32,
    PendingOwner,
    CallFrame32,
    [u8; STATE_SIZE],
    [u8; EXIT_SIZE],
);
fn pending(engine: &EngineInstance) -> Pending {
    let call = engine.pending_call.as_ref().unwrap();
    (call.token, call.owner, call.frame, call.state, call.exit)
}

struct Saved {
    arena: Vec<u8>,
    arena_pointer: usize,
    call: Pending,
    token: u32,
    rows: Vec<PageRange>,
    identity: u64,
    pages: u32,
    ram: Vec<Vec<u8>>,
    versions: Vec<CodeSnapshot>,
    modules: [(Vec<u8>, usize); 3],
    error: u32,
}

fn saved(f: &Fixture) -> Saved {
    let engine = &f.engine;
    let memory = engine.memory.as_ref().unwrap();
    let artifact = engine.artifact_bytes().unwrap();
    let caller = engine
        .guard_resident_unit(KEY, f.caller)
        .unwrap()
        .wasm_bytes(memory)
        .unwrap();
    let keeper = engine
        .guard_resident_unit(KEY, f.keeper)
        .unwrap()
        .wasm_bytes(memory)
        .unwrap();
    Saved {
        arena: engine.arena().to_vec(),
        arena_pointer: engine.arena_address(),
        call: pending(engine),
        token: engine.call_token,
        rows: engine.virtual_allocations.clone(),
        identity: memory.identity(),
        pages: memory.mapped_pages(),
        ram: PAGES
            .iter()
            .map(|&address| {
                let mut bytes = vec![0; 4096];
                memory.read(GuestAddress(address), &mut bytes).unwrap();
                bytes
            })
            .collect(),
        versions: PAGES
            .iter()
            .map(|&address| memory.snapshot_code(GuestAddress(address), 4096).unwrap())
            .collect(),
        modules: [
            (artifact.to_vec(), artifact.as_ptr() as usize),
            (caller.to_vec(), caller.as_ptr() as usize),
            (keeper.to_vec(), keeper.as_ptr() as usize),
        ],
        error: last_error(engine),
    }
}

fn unchanged(f: &Fixture, before: &Saved) {
    let after = saved(f);
    assert_eq!(after.arena, before.arena);
    assert_eq!(after.arena_pointer, before.arena_pointer);
    assert_eq!(after.call, before.call);
    assert_eq!(after.token, before.token);
    assert_eq!(after.rows, before.rows);
    assert_eq!(after.identity, before.identity);
    assert_eq!(after.pages, before.pages);
    assert_eq!(after.ram, before.ram);
    assert_eq!(after.modules, before.modules);
    assert_eq!(after.error, before.error);
    for version in &before.versions {
        assert!(f.engine.memory.as_ref().unwrap().is_code_current(version));
    }
}

#[test]
fn release_internal_encoding_and_version_failures_preserve_every_published_owner() {
    for exhausted in [false, true] {
        let mut f = fixture();
        assert_eq!(allocate(&mut f.engine, 4097), BASE);
        f.engine.protect(BASE, 2, 7).unwrap();
        let (token, state) = capture(
            &mut f.engine,
            CODE + 16,
            STACK,
            WindowsApi32::VirtualFree,
            &[BASE, 0, 0x8000],
        );
        if exhausted {
            f.engine
                .memory
                .as_mut()
                .unwrap()
                .exhaust_versions_for_test();
        } else {
            let mut invalid = state;
            invalid.eflags = 0;
            f.engine.pending_call.as_mut().unwrap().frame = CallFrame32::capture(
                f.engine.memory.as_ref().unwrap(),
                invalid,
                CallingConvention32::Stdcall,
                3,
            )
            .unwrap();
        }
        let before = saved(&f);
        let error = if exhausted {
            HostError::Memory(MemoryError::VersionExhausted)
        } else {
            HostError::Infrastructure
        };
        assert_eq!(complete(&mut f.engine, token), Err(error));
        unchanged(&f, &before);
        if !exhausted {
            f.engine.pending_call.as_mut().unwrap().frame = CallFrame32::capture(
                f.engine.memory.as_ref().unwrap(),
                state,
                CallingConvention32::Stdcall,
                3,
            )
            .unwrap();
            complete(&mut f.engine, token).unwrap();
            assert!(f.engine.virtual_allocations.is_empty());
            assert_eq!(f.engine.memory().unwrap().mapped_pages(), 3);
            assert!(f.engine.pending_call.is_none());
            assert_eq!(last_error(&f.engine), 8);
        }
    }
}

#[test]
fn private_return_can_complete_after_releasing_current_code_or_captured_stack() {
    for code in [false, true] {
        let mut f = fixture();
        assert_eq!(allocate(&mut f.engine, 1), BASE);
        let (pc, esp) = if code {
            f.engine.protect(BASE, 1, 7).unwrap();
            f.engine
                .memory
                .as_mut()
                .unwrap()
                .write(GuestAddress(BASE), &[0x0f, 0x0b])
                .unwrap();
            words(
                &mut f.engine,
                &[BASE, 2, BASE, WindowsApi32::VirtualFree.id()],
            );
            f.engine.compile_with_gates(1, 1).unwrap();
            (BASE, STACK)
        } else {
            (CODE + 16, BASE + 128)
        };
        let generation = f.engine.generation();
        let (token, before) = capture(
            &mut f.engine,
            pc,
            esp,
            WindowsApi32::VirtualFree,
            &[BASE, 0, 0x8000],
        );
        complete(&mut f.engine, token).unwrap();
        let state =
            decode_state(&f.engine.arena()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE]).unwrap();
        let mut expected = before;
        expected.registers[0] = 1;
        expected.registers[4] = esp + 16;
        expected.eip = RETURN;
        assert_eq!(state, expected);
        assert_eq!(
            decode_exit(&f.engine.arena()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE]).unwrap(),
            ExecutionExit {
                retired: 0,
                reason: ExitReason::NeedCode
            }
        );
        assert!(f.engine.pending_call.is_none());
        assert!(f.engine.virtual_allocations.is_empty());
        assert_eq!(f.engine.memory().unwrap().mapped_pages(), 3);
        assert!(matches!(
            f.engine
                .memory()
                .unwrap()
                .read(GuestAddress(BASE), &mut [0]),
            Err(MemoryError::Fault(_))
        ));
        f.engine.guard_resident_unit(KEY, f.caller).unwrap();
        f.engine.guard_resident_unit(KEY, f.keeper).unwrap();
        if code {
            assert_eq!(
                f.engine.guard_artifact(KEY, generation),
                Err(HostError::CodeInvalidated)
            );
        } else {
            f.engine.guard_artifact(KEY, generation).unwrap();
        }
        assert_eq!(last_error(&f.engine), 8);
    }
}

#[test]
fn failed_host_unmap_keeps_rows_and_partial_success_revokes_only_overlap() {
    let mut f = fixture();
    assert_eq!(allocate(&mut f.engine, 4097), BASE);
    let other = BASE + 65536;
    assert_eq!(allocate(&mut f.engine, 1), other);
    let first = PageRange::new(GuestAddress(BASE), 2).unwrap();
    let second = PageRange::new(GuestAddress(other), 1).unwrap();
    assert_eq!(f.engine.virtual_allocations, vec![first, second]);
    f.engine.protect(BASE, 2, 7).unwrap();
    f.engine.protect(other, 1, 7).unwrap();
    f.engine.write32(BASE, 0x1357_9bdf).unwrap();
    let memory = f.engine.memory().unwrap();
    let identity = memory.identity();
    let version = memory.snapshot_code(GuestAddress(BASE), 8192).unwrap();
    let other_version = memory.snapshot_code(GuestAddress(other), 4096).unwrap();
    let arena = f.engine.arena().to_vec();
    assert_eq!(
        f.engine.unmap(BASE, 3),
        Err(HostError::Memory(MemoryError::NotMapped {
            address: GuestAddress(BASE + 8192)
        }))
    );
    assert_eq!(f.engine.arena(), arena);
    assert_eq!(f.engine.virtual_allocations, vec![first, second]);
    assert_eq!(f.engine.memory().unwrap().identity(), identity);
    assert_eq!(f.engine.memory().unwrap().mapped_pages(), 6);
    assert!(f.engine.memory().unwrap().is_code_current(&version));
    assert!(f.engine.memory().unwrap().is_code_current(&other_version));
    f.engine.unmap(BASE + 4096, 1).unwrap();
    assert_eq!(f.engine.virtual_allocations, vec![second]);
    f.engine.map(BASE + 4096, 1, 7).unwrap();
    f.engine.write32(BASE + 4096, 0x2468_ace0).unwrap();
    assert_eq!(release(&mut f.engine, BASE), 0);
    assert_eq!(last_error(&f.engine), 487);
    assert_eq!(f.engine.virtual_allocations, vec![second]);
    let memory = f.engine.memory().unwrap();
    for (address, value) in [(BASE, 0x1357_9bdf_u32), (BASE + 4096, 0x2468_ace0)] {
        let mut bytes = [0; 4];
        memory.read(GuestAddress(address), &mut bytes).unwrap();
        assert_eq!(bytes, value.to_le_bytes());
    }
    assert!(memory.is_code_current(&other_version));
    assert_eq!(release(&mut f.engine, other), 1);
    assert!(f.engine.virtual_allocations.is_empty());
    assert_eq!(last_error(&f.engine), 487);
}

#[test]
fn terminal_retains_allocation_rows_and_buffer_until_close_drops_both() {
    let mut f = fixture();
    assert_eq!(allocate(&mut f.engine, 4097), BASE);
    let rows = f.engine.virtual_allocations.clone();
    let pointer = f.engine.virtual_allocations.as_ptr();
    let capacity = f.engine.virtual_allocations.capacity();
    let identity = f.engine.memory().unwrap().identity();
    assert!(capacity > 0);
    let (token, _) = capture(
        &mut f.engine,
        CODE + 32,
        STACK,
        WindowsApi32::ExitProcess,
        &[u32::MAX],
    );
    complete(&mut f.engine, token).unwrap();
    assert_eq!(f.engine.virtual_allocations, rows);
    assert_eq!(f.engine.virtual_allocations.as_ptr(), pointer);
    assert_eq!(f.engine.virtual_allocations.capacity(), capacity);
    assert_eq!(f.engine.memory.as_ref().unwrap().identity(), identity);
    assert_eq!(f.engine.memory.as_ref().unwrap().mapped_pages(), 5);
    assert!(matches!(f.engine.memory(), Err(HostError::ProcessExited)));
    assert_eq!(f.engine.exit_code, Some(u32::MAX));
    assert!(f.engine.pending_call.is_none());
    f.engine.close();
    assert!(f.engine.virtual_allocations.is_empty());
    assert_eq!(f.engine.virtual_allocations.capacity(), 0);
    assert!(f.engine.memory.is_none());
}

#[test]
fn pristine_image_admission_checks_empty_ledger_before_loader_or_memory_replacement() {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    let identity = engine.memory().unwrap().identity();
    let arena = engine.arena().to_vec();
    let row = PageRange::new(GuestAddress(BASE), 1).unwrap();
    // isolated defensive invariant injection; ordinary allocations also map pages.
    engine.virtual_allocations.push(row);
    assert_eq!(engine.image_capacity(), Err(HostError::InvalidRequest));
    assert_eq!(engine.load_pe32(b"bad"), Err(HostError::InvalidRequest));
    assert_eq!(engine.begin_image_input(1), Err(HostError::InvalidRequest));
    assert_eq!(engine.virtual_allocations, vec![row]);
    assert_eq!(engine.memory().unwrap().identity(), identity);
    assert_eq!(engine.memory().unwrap().mapped_pages(), 0);
    assert_eq!(engine.arena(), arena);
    assert!(engine.image_input.is_none());
    engine.virtual_allocations.clear();
    assert_eq!(engine.image_capacity(), Ok(1));
    assert_eq!(
        engine.load_pe32(b"bad"),
        Err(HostError::Loader(crate::loader::LoadError::Malformed))
    );
    assert_eq!(engine.memory().unwrap().identity(), identity);
}
