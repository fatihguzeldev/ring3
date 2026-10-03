use ring3_engine::{
    abi::x86::{decode_exit, decode_state, encode_exit_v3, encode_state},
    cpu::{ExecutionExit, ExitReason, x86::State32},
    memory::{Access, FaultReason, GuestAddress, MemoryError, MemoryFault},
    process::{CallError, EngineInstance, HostError, ResidentInstallation},
    windows::CallingConvention32,
};

const BASE: u32 = 0x0040_0000;
const ENTRY: u32 = BASE + 0x1000;
const KEY: u64 = 0x1020_3040_5060_7080;
const SELECTED: u32 = 0x0050_0000;
const GATE: u32 = 0x7000_0000;
const STACK: u32 = 0x8000;

fn put16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn image() -> Vec<u8> {
    // literal one-section pe32: two mapped pages and a declared sparse image tail.
    let mut bytes = vec![0; 0x400];
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 0x3c, 0x80);
    bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
    put16(&mut bytes, 0x84, 0x14c);
    put16(&mut bytes, 0x86, 1);
    put16(&mut bytes, 0x94, 224);
    put16(&mut bytes, 0x96, 0x0103);
    put16(&mut bytes, 0x98, 0x10b);
    for (offset, value) in [
        (0xa8, 0x1000),
        (0xb4, BASE),
        (0xb8, 4096),
        (0xbc, 512),
        (0xd0, 0x4000),
        (0xd4, 512),
        (0xf4, 16),
        (0x180, 3),
        (0x184, 0x1000),
        (0x188, 512),
        (0x18c, 512),
        (0x19c, 0x6000_0020),
    ] {
        put32(&mut bytes, offset, value);
    }
    put16(&mut bytes, 0xdc, 3);
    bytes[0x178..0x180].copy_from_slice(b".text\0\0\0");
    bytes[0x200..0x203].copy_from_slice(&[0x90, 0xeb, 0]);
    bytes
}

fn relocated_image() -> Vec<u8> {
    let mut bytes = image();
    bytes.resize(0x600, 0);
    put16(&mut bytes, 0x86, 2);
    put16(&mut bytes, 0x96, 0x0102);
    put32(&mut bytes, 0x180, 7);
    bytes[0x200..0x207].copy_from_slice(&[0xb8, 0, 0, 0x40, 0, 0xeb, 0]);
    bytes[0x1a0..0x1a8].copy_from_slice(b".reloc\0\0");
    for (offset, value) in [
        (0x1a8, 12),
        (0x1ac, 0x2000),
        (0x1b0, 512),
        (0x1b4, 0x400),
        (0x1c4, 0x4000_0040),
        (0x120, 0x2000),
        (0x124, 12),
        (0x400, 0x1000),
        (0x404, 12),
    ] {
        put32(&mut bytes, offset, value);
    }
    put16(&mut bytes, 0x408, 0x3001);
    bytes
}

fn linked_image() -> Vec<u8> {
    let mut bytes = relocated_image();
    bytes.resize(0x800, 0);
    put16(&mut bytes, 0x86, 3);
    bytes[0x1c8..0x1d0].copy_from_slice(b".idata\0\0");
    for (offset, value) in [
        (0x1d0, 0x180),
        (0x1d4, 0x3000),
        (0x1d8, 512),
        (0x1dc, 0x600),
        (0x1ec, 0x4000_0040),
        (0x100, 0x3000),
        (0x104, 40),
        (0x600, 0x3040),
        (0x60c, 0x30c0),
        (0x610, 0x3080),
        (0x640, 0x3100),
        (0x680, 0x3100),
    ] {
        put32(&mut bytes, offset, value);
    }
    bytes[0x6c0..0x6cd].copy_from_slice(b"KERNEL32.dll\0");
    bytes[0x702..0x70f].copy_from_slice(b"GetLastError\0");
    bytes
}

fn fixture(pages: u32) -> EngineInstance {
    let mut engine = EngineInstance::new(pages, KEY).unwrap();
    engine.load_pe32(&image()).unwrap();
    engine
}

fn read(engine: &EngineInstance, address: u32, length: usize) -> Result<Vec<u8>, HostError> {
    let mut bytes = vec![0; length];
    engine
        .memory()?
        .read(GuestAddress(address), &mut bytes)
        .map_err(HostError::Memory)?;
    Ok(bytes)
}

fn fault(address: u32, access: Access, reason: FaultReason) -> MemoryError {
    MemoryError::Fault(MemoryFault {
        address: GuestAddress(address),
        access,
        reason,
    })
}

fn startup_bytes(entry: u32, esp: u32) -> [u8; 96] {
    // independently authored complete startup publication, including both codec headers.
    let mut bytes = [0; 96];
    bytes[..4].copy_from_slice(b"R3ST");
    bytes[56..60].copy_from_slice(b"R3EX");
    for (offset, value) in [
        (4, 0x10001),
        (8, 56),
        (32, esp),
        (48, entry),
        (52, 2),
        (60, 0x10003),
        (64, 40),
        (72, 3),
    ] {
        put32(&mut bytes, offset, value);
    }
    bytes
}

fn transfer_entries(engine: &mut EngineInstance, entries: &[u32], gates: &[(u32, u32)]) {
    let transfer = &mut engine.arena_mut().unwrap()[140..];
    transfer.fill(0xa5);
    for (index, entry) in entries.iter().enumerate() {
        put32(transfer, index * 4, *entry);
    }
    for (index, (entry, id)) in gates.iter().enumerate() {
        let offset = entries.len() * 4 + index * 8;
        put32(transfer, offset, *entry);
        put32(transfer, offset + 4, *id);
    }
}

fn upload(engine: &mut EngineInstance, address: u32, bytes: &[u8]) {
    engine.arena_mut().unwrap()[140..140 + bytes.len()].copy_from_slice(bytes);
    engine.upload(address, bytes.len() as u32).unwrap();
}

#[derive(Debug, PartialEq, Eq)]
struct UnitSnapshot {
    bytes: Result<Vec<u8>, HostError>,
    pointer: Option<usize>,
    guard: Result<(), HostError>,
}

#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    arena: Vec<u8>,
    arena_address: usize,
    mapped_pages: Result<u32, HostError>,
    generation: u32,
    artifact: Result<Vec<u8>, HostError>,
    artifact_pointer: Option<usize>,
    dispatcher: Result<Vec<u8>, HostError>,
    ram: Vec<Result<Vec<u8>, HostError>>,
    units: Vec<UnitSnapshot>,
    installations: Vec<Result<ResidentInstallation, HostError>>,
}

fn snapshot(engine: &EngineInstance, ids: &[u64]) -> Snapshot {
    Snapshot {
        arena: engine.arena().to_vec(),
        arena_address: engine.arena_address(),
        mapped_pages: engine.memory().map(|memory| memory.mapped_pages()),
        generation: engine.generation(),
        artifact: engine.artifact_bytes().map(<[u8]>::to_vec),
        artifact_pointer: engine
            .artifact_bytes()
            .ok()
            .map(|bytes| bytes.as_ptr() as usize),
        dispatcher: engine.dispatcher_bytes(KEY).map(<[u8]>::to_vec),
        ram: [
            0,
            STACK,
            STACK + 4096,
            BASE,
            ENTRY,
            BASE + 0x2000,
            BASE + 0x3000,
            SELECTED,
            SELECTED + 0x1000,
            SELECTED + 0x2000,
            SELECTED + 0x3000,
            GATE,
            0xffff_f000,
        ]
        .map(|address| read(engine, address, 4096))
        .to_vec(),
        units: ids
            .iter()
            .map(|&id| UnitSnapshot {
                bytes: engine.resident_bytes(id).map(<[u8]>::to_vec),
                pointer: engine
                    .resident_bytes(id)
                    .ok()
                    .map(|bytes| bytes.as_ptr() as usize),
                guard: engine.guard_resident(KEY, id),
            })
            .collect(),
        installations: [ENTRY, SELECTED + 0x1000, GATE]
            .map(|address| engine.lookup_installed_resident(KEY, address))
            .to_vec(),
    }
}

fn failed(engine: &mut EngineInstance, ids: &[u64], expected: HostError, base: u32, pages: u32) {
    let before = snapshot(engine, ids);
    assert_eq!(engine.start_loaded_image(base, pages), Err(expected));
    assert_eq!(snapshot(engine, ids), before);
}

#[test]
fn loaded_image_starts_at_private_entry_with_empty_zero_stack_and_need_code() {
    let mut engine = EngineInstance::new(4, 7).unwrap();
    let metadata = engine.load_pe32(&image()).unwrap();
    assert_eq!(metadata.entry_point, ENTRY);
    assert_eq!(metadata.mapped_pages, 2);
    engine.start_loaded_image(0x8000, 2).unwrap();
    assert_eq!(
        decode_state(&engine.arena()[..56]).unwrap(),
        State32 {
            registers: [0, 0, 0, 0, 0xa000, 0, 0, 0],
            eip: ENTRY,
            eflags: 2
        },
    );
    assert_eq!(
        decode_exit(&engine.arena()[56..96]).unwrap(),
        ExecutionExit {
            retired: 0,
            reason: ExitReason::NeedCode
        }
    );
    assert_eq!(engine.memory().unwrap().mapped_pages(), 4);
}

#[test]
fn all_three_loader_paths_use_private_selected_entry_and_preserve_other_arena_bytes() {
    for profile in 0..3 {
        let mut engine = EngineInstance::new(8, KEY).unwrap();
        let (entry, image_pages) = match profile {
            0 => {
                let metadata = engine.load_pe32(&image()).unwrap();
                (metadata.entry_point, metadata.mapped_pages)
            }
            1 => {
                let metadata = engine.load_pe32_at(&relocated_image(), SELECTED).unwrap();
                (metadata.entry_point, metadata.mapped_pages)
            }
            _ => {
                let metadata = engine
                    .load_pe32_linked_at(&linked_image(), SELECTED, GATE)
                    .unwrap();
                (metadata.image.entry_point, metadata.image.mapped_pages)
            }
        };
        assert_eq!(
            entry,
            if profile == 0 {
                ENTRY
            } else {
                SELECTED + 0x1000
            }
        );
        assert_eq!(image_pages, [2, 3, 5][profile]);
        engine.arena_mut().unwrap()[100..].fill(0xa5);
        // forged transfer entry/base/stack values have no startup authority.
        put32(engine.arena_mut().unwrap(), 140 + 24, 0xffff_ffff);
        let before = engine.arena().to_vec();
        let arena_address = engine.arena_address();
        let code = engine
            .memory()
            .unwrap()
            .snapshot_code(GuestAddress(entry), 1)
            .unwrap();
        engine.start_loaded_image(STACK, 2).unwrap();
        let mut expected = before;
        expected[..96].copy_from_slice(&startup_bytes(entry, 0xa000));
        assert_eq!(engine.arena(), expected);
        assert_eq!(engine.arena_address(), arena_address);
        assert_eq!(engine.memory().unwrap().mapped_pages(), image_pages + 2);
        assert!(engine.memory().unwrap().is_code_current(&code));
        assert_eq!(read(&engine, STACK, 8192).unwrap(), vec![0; 8192]);
        for address in [STACK, 0x9fff] {
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .resolve(GuestAddress(address), Access::Read)
                    .is_ok()
            );
            assert!(
                engine
                    .memory()
                    .unwrap()
                    .resolve(GuestAddress(address), Access::Write)
                    .is_ok()
            );
            assert_eq!(
                engine
                    .memory()
                    .unwrap()
                    .resolve(GuestAddress(address), Access::Execute),
                Err(fault(address, Access::Execute, FaultReason::Permission))
            );
        }
        assert_eq!(
            engine
                .memory()
                .unwrap()
                .resolve(GuestAddress(0xa000), Access::Read),
            Err(fault(0xa000, Access::Read, FaultReason::Unmapped))
        );
    }
}

#[test]
fn precompiled_artifact_resident_installation_and_dispatcher_keep_their_owners() {
    let mut engine = fixture(4);
    transfer_entries(&mut engine, &[ENTRY], &[]);
    engine.compile_entries(1, 0).unwrap();
    let id = engine.compile_resident_entries(1, 0).unwrap().get();
    engine
        .acknowledge_resident_installation(KEY, id, 0)
        .unwrap();
    let before = snapshot(&engine, &[id]);
    let code = engine
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(ENTRY), 3)
        .unwrap();
    engine.start_loaded_image(STACK, 2).unwrap();
    let after = snapshot(&engine, &[id]);
    assert_eq!(after.units, before.units);
    assert_eq!(after.installations, before.installations);
    assert_eq!(after.generation, before.generation);
    assert_eq!(after.artifact, before.artifact);
    assert_eq!(after.artifact_pointer, before.artifact_pointer);
    assert_eq!(after.dispatcher, before.dispatcher);
    assert_eq!(after.arena_address, before.arena_address);
    assert_eq!(&after.arena[96..], &before.arena[96..]);
    assert!(engine.memory().unwrap().is_code_current(&code));
    assert_eq!(engine.guard_resident(KEY, id), Ok(()));
}

#[test]
fn stack_base_zero_and_upper_limit_have_exact_wrapped_esp_and_permissions() {
    for (base, esp) in [(0, 4096), (0xffff_f000, 0)] {
        let mut engine = fixture(3);
        let before = engine.arena().to_vec();
        engine.start_loaded_image(base, 1).unwrap();
        let mut expected = before;
        expected[..96].copy_from_slice(&startup_bytes(ENTRY, esp));
        assert_eq!(engine.arena(), expected);
        assert_eq!(read(&engine, base, 4096).unwrap(), vec![0; 4096]);
        let last = base + 4095;
        assert!(
            engine
                .memory()
                .unwrap()
                .resolve(GuestAddress(last), Access::Write)
                .is_ok()
        );
        assert_eq!(
            engine
                .memory()
                .unwrap()
                .resolve(GuestAddress(last), Access::Execute),
            Err(fault(last, Access::Execute, FaultReason::Permission))
        );
    }
}

#[test]
fn whole_image_gaps_are_reserved_for_startup_but_exact_adjacency_is_allowed() {
    for (base, pages) in [
        (BASE - 4096, 2),
        (BASE, 1),
        (BASE + 0x2000, 1),
        (BASE + 0x3000, 2),
    ] {
        let mut engine = fixture(4);
        failed(&mut engine, &[], HostError::InvalidRequest, base, pages);
        engine.start_loaded_image(STACK, 1).unwrap();
    }
    for base in [BASE - 4096, BASE + 0x4000] {
        let mut engine = fixture(3);
        engine.start_loaded_image(base, 1).unwrap();
        assert_eq!(
            decode_state(&engine.arena()[..56]).unwrap().registers[4],
            base + 4096
        );
    }
}

#[test]
fn structural_errors_precede_entry_fault_cancel_and_mapping_capacity() {
    let mut engine = fixture(2);
    engine.unmap(ENTRY, 1).unwrap();
    engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
    for (base, pages) in [
        (1, 1),
        (STACK, 0),
        (STACK, 4097),
        (STACK, u32::MAX),
        (0xffff_f000, 2),
        (BASE + 0x2000, 1),
    ] {
        failed(&mut engine, &[], HostError::InvalidRequest, base, pages);
    }
    failed(
        &mut engine,
        &[],
        HostError::Memory(fault(ENTRY, Access::Execute, FaultReason::Unmapped)),
        STACK,
        1,
    );
}

#[test]
fn current_private_entry_execute_check_precedes_cancel_and_supports_permission_retry() {
    let mut engine = fixture(4);
    engine.protect(ENTRY, 1, 1).unwrap();
    engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
    failed(
        &mut engine,
        &[],
        HostError::Memory(fault(ENTRY, Access::Execute, FaultReason::Permission)),
        STACK,
        2,
    );
    engine.protect(ENTRY, 1, 5).unwrap();
    failed(
        &mut engine,
        &[],
        HostError::Call(CallError::Cancelled),
        STACK,
        2,
    );
    engine.arena_mut().unwrap()[96..100].fill(0);
    engine.start_loaded_image(STACK, 2).unwrap();
}

#[test]
fn cancellation_precedes_existing_map_collision_and_capacity_without_clearing_the_flag() {
    let mut engine = fixture(4);
    engine.map(STACK, 1, 3).unwrap();
    upload(&mut engine, STACK, &[0x6d]);
    engine.arena_mut().unwrap()[96..100].copy_from_slice(&0xffff_ffff_u32.to_le_bytes());
    failed(
        &mut engine,
        &[],
        HostError::Call(CallError::Cancelled),
        STACK,
        2,
    );
    engine.arena_mut().unwrap()[96..100].fill(0);
    failed(
        &mut engine,
        &[],
        HostError::Memory(MemoryError::AlreadyMapped {
            address: GuestAddress(STACK),
        }),
        STACK,
        2,
    );
    assert_eq!(read(&engine, STACK, 1).unwrap(), [0x6d]);
    engine.unmap(STACK, 1).unwrap();
    engine.start_loaded_image(STACK, 2).unwrap();
}

#[test]
fn partial_collision_and_capacity_failure_keep_pages_unmapped_and_allow_valid_retry() {
    let mut collision = fixture(4);
    collision.map(STACK + 4096, 1, 3).unwrap();
    upload(&mut collision, STACK + 4096, &[0xb3]);
    failed(
        &mut collision,
        &[],
        HostError::Memory(MemoryError::AlreadyMapped {
            address: GuestAddress(STACK + 4096),
        }),
        STACK,
        3,
    );
    assert_eq!(
        read(&collision, STACK, 1),
        Err(HostError::Memory(fault(
            STACK,
            Access::Read,
            FaultReason::Unmapped
        )))
    );
    collision.unmap(STACK + 4096, 1).unwrap();
    collision.start_loaded_image(STACK, 2).unwrap();

    let mut capacity = fixture(3);
    failed(
        &mut capacity,
        &[],
        HostError::Memory(MemoryError::Capacity),
        STACK,
        2,
    );
    failed(
        &mut capacity,
        &[],
        HostError::Memory(MemoryError::Capacity),
        0x8000_0000,
        4096,
    );
    capacity.start_loaded_image(STACK, 1).unwrap();
}

#[test]
fn mapped_external_gate_collision_is_preserved_and_unmapped_gate_is_not_reserved() {
    let mut engine = EngineInstance::new(6, KEY).unwrap();
    let metadata = engine
        .load_pe32_linked_at(&linked_image(), SELECTED, GATE)
        .unwrap();
    assert_eq!(metadata.gate_count, 1);
    assert_eq!(metadata.gates[0].entry.0, GATE);
    failed(
        &mut engine,
        &[],
        HostError::Memory(MemoryError::AlreadyMapped {
            address: GuestAddress(GATE),
        }),
        GATE,
        1,
    );
    engine.unmap(GATE, 1).unwrap();
    engine.start_loaded_image(GATE, 1).unwrap();
    assert_eq!(read(&engine, GATE, 4096).unwrap(), vec![0; 4096]);
    assert_eq!(
        decode_state(&engine.arena()[..56]).unwrap().eip,
        SELECTED + 0x1000
    );
}

#[test]
fn no_loaded_image_and_each_modified_initial_context_byte_reject_before_other_inputs() {
    let mut empty = EngineInstance::new(1, KEY).unwrap();
    failed(&mut empty, &[], HostError::InvalidRequest, 1, u32::MAX);
    for index in 0..96 {
        let mut engine = fixture(3);
        engine.arena_mut().unwrap()[index] ^= 0x80;
        failed(&mut engine, &[], HostError::InvalidRequest, 1, u32::MAX);
    }
    let mut engine = fixture(3);
    let initial = engine.arena()[..96].to_vec();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Budget,
        },
        &mut engine.arena_mut().unwrap()[56..96],
    )
    .unwrap();
    failed(&mut engine, &[], HostError::InvalidRequest, STACK, 1);
    engine.arena_mut().unwrap()[..96].copy_from_slice(&initial);
    engine.start_loaded_image(STACK, 1).unwrap();
}

fn pending_fixture() -> (EngineInstance, u32, Vec<u8>) {
    let mut engine = fixture(6);
    let initial = engine.arena()[..96].to_vec();
    engine.map(GATE, 1, 7).unwrap();
    engine.map(STACK, 1, 3).unwrap();
    upload(&mut engine, GATE, &[0x0f, 0x0b]);
    upload(&mut engine, GATE + 0x20, &[0x0f, 0x0b]);
    upload(&mut engine, STACK + 0x10, &ENTRY.to_le_bytes());
    let transfer = &mut engine.arena_mut().unwrap()[140..];
    for (index, value) in [ENTRY, 3, GATE, 2, GATE + 0x20, 2, GATE, 17, GATE + 0x20, 18]
        .into_iter()
        .enumerate()
    {
        put32(transfer, index * 4, value);
    }
    engine.compile_with_gates(3, 2).unwrap();
    // this synthetic native stop tests ownership; the actual fixture executes real calls.
    encode_state(
        &State32 {
            registers: [0, 0, 0, 0, STACK + 0x10, 0, 0, 0],
            eip: GATE,
            eflags: 2,
        },
        &mut engine.arena_mut().unwrap()[..56],
    )
    .unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 3,
            reason: ExitReason::Gate { id: 17 },
        },
        &mut engine.arena_mut().unwrap()[56..96],
    )
    .unwrap();
    let token = engine
        .capture_call(KEY, 1, CallingConvention32::Cdecl, 0)
        .unwrap()
        .token;
    (engine, token, initial)
}

#[test]
fn pending_and_callback_busy_precede_image_context_structural_and_cancel_checks() {
    let (mut engine, token, _) = pending_fixture();
    failed(
        &mut engine,
        &[],
        HostError::Call(CallError::Busy),
        1,
        u32::MAX,
    );
    let callback = engine
        .begin_callback(KEY, 1, token, ENTRY, GATE + 0x20, 18, &[])
        .unwrap();
    engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
    failed(&mut engine, &[], HostError::Call(CallError::Busy), 1, 0);
    engine.arena_mut().unwrap()[96..100].fill(0);
    engine.abort_callback(KEY, callback.token).unwrap();
    engine.complete_call(KEY, 1, token, 0).unwrap();
}

#[test]
fn prior_captured_call_token_rejects_even_after_exact_manual_context_restoration() {
    let (mut engine, token, initial) = pending_fixture();
    engine.complete_call(KEY, 1, token, 0).unwrap();
    engine.arena_mut().unwrap()[..96].copy_from_slice(&initial);
    failed(&mut engine, &[], HostError::InvalidRequest, 0xa000, 1);
}

#[test]
fn successful_start_latch_survives_context_restore_stack_and_image_unmap() {
    let mut engine = fixture(4);
    let initial = engine.arena()[..96].to_vec();
    engine.start_loaded_image(STACK, 2).unwrap();
    engine.arena_mut().unwrap()[..96].copy_from_slice(&initial);
    engine.unmap(STACK, 2).unwrap();
    failed(&mut engine, &[], HostError::InvalidRequest, 0xa000, 1);
    engine.unmap(BASE, 1).unwrap();
    engine.unmap(ENTRY, 1).unwrap();
    failed(&mut engine, &[], HostError::InvalidRequest, STACK, 1);
    assert_eq!(engine.load_pe32(&image()), Err(HostError::InvalidRequest));
}

#[test]
fn closed_wins_before_context_latch_cancel_and_structural_inputs_preserving_tombstone() {
    for started in [false, true] {
        let mut engine = fixture(3);
        if started {
            engine.start_loaded_image(STACK, 1).unwrap();
        }
        engine.arena_mut().unwrap()[0] ^= 0xff;
        engine.arena_mut().unwrap()[96..100].copy_from_slice(&1_u32.to_le_bytes());
        engine.close();
        failed(&mut engine, &[], HostError::Closed, 1, u32::MAX);
    }
}
