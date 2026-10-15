use super::*;
use crate::{cpu::dbt::RegistryError, process::CallError};

#[test]
fn exhausted_versions_preserve_resident_store_ram_and_executing_snapshot() {
    let mut instance = EngineInstance::new(2, 7).unwrap();
    instance.map(0x1000, 1, 7).unwrap();
    instance.map(0x4000, 1, 7).unwrap();
    instance.write32(0x1000, 0x00eb_0389).unwrap();
    instance.write32(0x4000, 0x4433_2211).unwrap();
    instance.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0, 0x10, 0, 0, 4, 0, 0, 0]);
    let id = instance.compile_resident(1).unwrap().get();
    let code_snapshot = instance
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(0x1000), 4)
        .unwrap();
    let data_snapshot = instance
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(0x4000), 4)
        .unwrap();
    let unit = instance.resident_bytes(id).unwrap().to_vec();
    let pointer = instance.resident_bytes(id).unwrap().as_ptr();
    let mut expected_arena = instance.arena().to_vec();
    expected_arena[100..140].copy_from_slice(&[
        0x52, 0x33, 0x4d, 0x48, 1, 0, 1, 0, 40, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 1, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ]);
    instance
        .memory
        .as_mut()
        .unwrap()
        .exhaust_versions_for_test();

    for (address, value) in [(0x4000, u32::MAX), (0x1000, 0x00eb_0390)] {
        assert_eq!(
            instance.store_resident32(7, id, address, value),
            Ok(StoreCompletion::Complete)
        );
        assert_eq!(instance.arena(), expected_arena);
    }
    assert_eq!(instance.generation(), 0);
    assert_eq!(instance.artifact_bytes(), Err(HostError::InvalidArtifact));
    assert_eq!(instance.guard_resident(7, id), Ok(()));
    assert_eq!(instance.resident_bytes(id).unwrap(), unit);
    assert_eq!(instance.resident_bytes(id).unwrap().as_ptr(), pointer);
    let memory = instance.memory().unwrap();
    assert!(memory.is_code_current(&code_snapshot));
    assert!(memory.is_code_current(&data_snapshot));
    for (address, expected) in [
        (0x1000, [0x89, 0x03, 0xeb, 0]),
        (0x4000, [0x11, 0x22, 0x33, 0x44]),
    ] {
        let mut actual = [0; 4];
        memory.read(GuestAddress(address), &mut actual).unwrap();
        assert_eq!(actual, expected);
    }
}

#[test]
fn exhausted_memory_versions_complete_store_without_committing_or_invalidating() {
    let mut instance = EngineInstance::new(2, 7).unwrap();
    instance.map(0x1000, 1, 7).unwrap();
    instance.map(0x4000, 1, 7).unwrap();
    instance.write32(0x1000, 0x9090_9090).unwrap();
    instance.write32(0x4000, 0x4433_2211).unwrap();
    instance.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0, 0x10, 0, 0, 1, 0, 0, 0]);
    let generation = instance.compile(1).unwrap();
    let code_snapshot = instance
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(0x1000), 4)
        .unwrap();
    let data_snapshot = instance
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(0x4000), 4)
        .unwrap();
    let artifact = instance.artifact_bytes().unwrap().to_vec();
    let mut expected_arena = instance.arena().to_vec();
    expected_arena[100..140].copy_from_slice(&[
        0x52, 0x33, 0x4d, 0x48, 1, 0, 1, 0, 40, 0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 1, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ]);
    instance
        .memory
        .as_mut()
        .unwrap()
        .exhaust_versions_for_test();

    assert_eq!(
        instance.store32(0x4000, u32::MAX),
        Ok(StoreCompletion::Complete)
    );
    assert_eq!(instance.arena(), expected_arena);
    assert_eq!(instance.generation(), generation);
    assert_eq!(instance.artifact_bytes().unwrap(), artifact);
    assert_eq!(instance.guard(7, generation), Ok(()));
    let memory = instance.memory().unwrap();
    assert!(memory.is_code_current(&code_snapshot));
    assert!(memory.is_code_current(&data_snapshot));
    for (address, expected) in [(0x1000, [0x90; 4]), (0x4000, [0x11, 0x22, 0x33, 0x44])] {
        let mut bytes = [0; 4];
        memory.read(GuestAddress(address), &mut bytes).unwrap();
        assert_eq!(bytes, expected);
    }
}

fn byte_store_fixture() -> (EngineInstance, u64, u64) {
    let mut engine = EngineInstance::new(4, 0x1234_5678_9abc_def0).unwrap();
    for address in [0x1000, 0x3000, 0x5000, 0x8000] {
        engine.map(address, 1, 7).unwrap();
    }
    for (address, bytes) in [
        (0x1000, &[0x0f, 0x0b][..]),
        (0x3000, &[0x90, 0xeb, 0][..]),
        (0x5000, &[0x11, 0x22, 0x33, 0x44][..]),
        (0x8000, &[0, 0x30, 0, 0, 0xef, 0xcd, 0xab, 0x89][..]),
    ] {
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(address), bytes)
            .unwrap();
    }
    let transfer = &mut engine.arena_mut().unwrap()[140..164];
    for (index, word) in [0x1000_u32, 2, 0x3000, 3, 0x1000, 7]
        .into_iter()
        .enumerate()
    {
        transfer[index * 4..index * 4 + 4].copy_from_slice(&word.to_le_bytes());
    }
    engine.compile_with_gates(2, 1).unwrap();
    for (index, word) in [0x1000_u32, 2, 0x1000, 7].into_iter().enumerate() {
        engine.arena_mut().unwrap()[140 + index * 4..144 + index * 4]
            .copy_from_slice(&word.to_le_bytes());
    }
    let outer = engine.compile_resident_with_gates(1, 1).unwrap().get();
    engine.arena_mut().unwrap()[140..148].copy_from_slice(&[0, 0x30, 0, 0, 3, 0, 0, 0]);
    let home = engine.compile_resident(1).unwrap().get();
    (engine, outer, home)
}

fn byte_store_packet(version: u16, fields: [u32; 6]) -> [u8; 40] {
    let mut bytes = [0; 40];
    bytes[..16].copy_from_slice(&[82, 51, 77, 72, 0, 0, 1, 0, 40, 0, 0, 0, 0, 0, 0, 0]);
    bytes[4..6].copy_from_slice(&version.to_le_bytes());
    for (index, field) in fields.into_iter().enumerate() {
        bytes[16 + index * 4..20 + index * 4].copy_from_slice(&field.to_le_bytes());
    }
    bytes
}

fn byte_store_helper_only(engine: &EngineInstance, before: &[u8], version: u16, fields: [u32; 6]) {
    let mut expected = before.to_vec();
    expected[100..140].copy_from_slice(&byte_store_packet(version, fields));
    assert_eq!(engine.arena(), expected);
}

fn byte_store_ram(engine: &EngineInstance, address: u32, length: usize) -> Vec<u8> {
    let mut bytes = vec![0; length];
    engine
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(address), &mut bytes)
        .unwrap();
    bytes
}

fn byte_store_inject_gate(engine: &mut EngineInstance) {
    // canonical native stop model; actual call execution is proved by the wasm fixture.
    let state = crate::cpu::x86::State32 {
        registers: [1, 2, 3, 4, 0x8000, 6, 7, 8],
        eip: 0x1000,
        eflags: 0xcd7,
    };
    crate::abi::x86::encode_state(&state, &mut engine.arena_mut().unwrap()[..56]).unwrap();
    crate::abi::x86::encode_exit_v3(
        &crate::cpu::ExecutionExit {
            retired: 3,
            reason: crate::cpu::ExitReason::Gate { id: 7 },
        },
        &mut engine.arena_mut().unwrap()[56..96],
    )
    .unwrap();
}

#[test]
fn byte_store_version_exhaustion_preserves_ram_identity_snapshots_and_all_three_owners() {
    let (mut engine, outer, home) = byte_store_fixture();
    let generation = engine.generation();
    let artifact = engine.artifact_bytes().unwrap().to_vec();
    let artifact_pointer = engine.artifact_bytes().unwrap().as_ptr();
    let resident = engine.resident_bytes(home).unwrap().to_vec();
    let resident_pointer = engine.resident_bytes(home).unwrap().as_ptr();
    let identity = engine.memory().unwrap().identity();
    let snapshots = [0x1000, 0x3000, 0x5000].map(|address| {
        engine
            .memory()
            .unwrap()
            .snapshot_code(GuestAddress(address), 2)
            .unwrap()
    });
    engine.memory.as_mut().unwrap().exhaust_versions_for_test();
    for address in [0x5001, 0x1000, 0x3000] {
        for method in 0..3 {
            let before = engine.arena().to_vec();
            match method {
                0 => engine.write8(address, u32::MAX).unwrap(),
                1 => assert_eq!(
                    engine.store8(address, u32::MAX),
                    Ok(StoreCompletion::Complete)
                ),
                _ => assert_eq!(
                    engine.store_resident8(engine.key, home, address, u32::MAX),
                    Ok(StoreCompletion::Complete)
                ),
            }
            byte_store_helper_only(&engine, &before, 3, [2, 0, 1, 0, 0, 1]);
        }
    }
    assert_eq!(byte_store_ram(&engine, 0x1000, 2), [0x0f, 0x0b]);
    assert_eq!(byte_store_ram(&engine, 0x3000, 3), [0x90, 0xeb, 0]);
    assert_eq!(byte_store_ram(&engine, 0x5000, 4), [0x11, 0x22, 0x33, 0x44]);
    assert_eq!(engine.memory().unwrap().identity(), identity);
    assert!(
        snapshots
            .iter()
            .all(|snapshot| engine.memory().unwrap().is_code_current(snapshot))
    );
    assert_eq!(engine.generation(), generation);
    assert_eq!(engine.artifact_bytes().unwrap(), artifact);
    assert_eq!(engine.artifact_bytes().unwrap().as_ptr(), artifact_pointer);
    assert_eq!(engine.resident_bytes(home).unwrap(), resident);
    assert_eq!(
        engine.resident_bytes(home).unwrap().as_ptr(),
        resident_pointer
    );
    engine.guard(engine.key, generation).unwrap();
    engine.guard_resident(engine.key, outer).unwrap();
    engine.guard_resident(engine.key, home).unwrap();
}

#[test]
fn byte_store_pending_is_busy_before_effect_while_direct_and_old_store4_remain_usable() {
    for resident in [false, true] {
        let (mut engine, outer, home) = byte_store_fixture();
        byte_store_inject_gate(&mut engine);
        if resident {
            engine
                .capture_resident_call(
                    engine.key,
                    outer,
                    crate::windows::CallingConvention32::Cdecl,
                    1,
                )
                .unwrap();
        } else {
            engine
                .capture_call(
                    engine.key,
                    engine.generation(),
                    crate::windows::CallingConvention32::Cdecl,
                    1,
                )
                .unwrap();
        }
        let token = engine.pending_call.as_ref().unwrap().token;
        let before = engine.arena().to_vec();
        let ram = byte_store_ram(&engine, 0x5000, 4);
        for address in [0x5000, 0xffff_ffff] {
            assert_eq!(
                engine.store8(address, 0xff),
                Err(HostError::Call(CallError::Busy))
            );
            assert_eq!(
                engine.store_resident8(engine.key, home, address, 0xff),
                Err(HostError::Call(CallError::Busy))
            );
            assert_eq!(engine.arena(), before);
            assert_eq!(byte_store_ram(&engine, 0x5000, 4), ram);
        }
        assert_eq!(
            engine.store_resident8(engine.key ^ (1 << 32), home, 0x5000, 0xff),
            Err(HostError::InvalidArtifact)
        );
        assert_eq!(engine.arena(), before);
        engine.write8(0x5001, 0x180).unwrap();
        byte_store_helper_only(&engine, &before, 3, [0, 0, 0, 0, 0, 1]);
        let before_store4 = engine.arena().to_vec();
        assert_eq!(
            engine.store32(0x5000, 0x8877_6655),
            Ok(StoreCompletion::Complete)
        );
        byte_store_helper_only(&engine, &before_store4, 1, [0, 0, 0, 0, 0, 0]);
        assert_eq!(byte_store_ram(&engine, 0x5000, 4), [0x55, 0x66, 0x77, 0x88]);
        assert_eq!(engine.pending_call.as_ref().unwrap().token, token);
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(0x1000), &[0x0f])
            .unwrap();
        let before_stale = engine.arena().to_vec();
        assert_eq!(engine.store8(0x5000, 0xff), Err(HostError::CodeInvalidated));
        assert_eq!(
            engine.store_resident8(engine.key, outer, 0x5000, 0xff),
            Err(HostError::Resident(RegistryError::CodeInvalidated))
        );
        assert_eq!(engine.arena(), before_stale);
    }
}

#[test]
fn byte_store_resident_callback_uses_only_authorized_active_current_owner() {
    use crate::process::calls::callback::{SuspendedCallback, SuspendedRecord};
    let (mut engine, outer_id, home_id) = byte_store_fixture();
    byte_store_inject_gate(&mut engine);
    engine
        .capture_resident_call(
            engine.key,
            outer_id,
            crate::windows::CallingConvention32::Cdecl,
            1,
        )
        .unwrap();
    let outer = engine.pending_call.take().unwrap();
    let token = outer.token + 1;
    // private native activity model; this is not callback execution or installation evidence.
    engine.callback = Some(SuspendedCallback {
        parent: None,
        outer,
        record: SuspendedRecord::Resident {
            record: crate::abi::resident_callback::ResidentCallbackRecord32 {
                token,
                outer_token: token - 1,
                phase: 1,
                outcome: 0,
                entry_pc: 0x3000,
                entry_esp: 0x8000,
                return_pc: 0x1000,
                return_id: 8,
                stack_words: 0,
                result: 0,
                outer_unit_id: outer_id,
                callback_unit_id: home_id,
            },
            authorized: false,
            active_unit_id: home_id,
        },
    });
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.store8(0x5000, 0xff),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.store_resident8(engine.key, home_id, 0x5000, 0xff),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    if let SuspendedRecord::Resident { authorized, .. } =
        &mut engine.callback.as_mut().unwrap().record
    {
        *authorized = true;
    }
    assert_eq!(
        engine.store8(0x5000, 0xff),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.store_resident8(engine.key, outer_id, 0x5000, 0xff),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(
        engine.store_resident8(engine.key, home_id, 0x5001, 0x180),
        Ok(StoreCompletion::Complete)
    );
    byte_store_helper_only(&engine, &before, 3, [0, 0, 0, 0, 0, 1]);
    assert_eq!(byte_store_ram(&engine, 0x5000, 4), [0x11, 0x80, 0x33, 0x44]);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1000), &[0x0f])
        .unwrap();
    let before_stale = engine.arena().to_vec();
    assert_eq!(
        engine.store_resident8(engine.key, home_id, 0x5000, 0xff),
        Err(HostError::Resident(RegistryError::CodeInvalidated))
    );
    assert_eq!(engine.arena(), before_stale);

    let (mut engine, _, home) = byte_store_fixture();
    byte_store_inject_gate(&mut engine);
    engine
        .capture_call(
            engine.key,
            engine.generation(),
            crate::windows::CallingConvention32::Cdecl,
            1,
        )
        .unwrap();
    let outer = engine.pending_call.take().unwrap();
    let token = outer.token + 1;
    engine.callback = Some(SuspendedCallback {
        parent: None,
        outer,
        record: SuspendedRecord::Replacement(crate::abi::callback::CallbackRecord32 {
            token,
            outer_token: token - 1,
            phase: 1,
            outcome: 0,
            entry_pc: 0x3000,
            entry_esp: 0x8000,
            return_pc: 0x1000,
            return_id: 8,
            stack_words: 0,
            result: 0,
            generation: engine.generation(),
        }),
    });
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.store_resident8(engine.key, home, 0x5000, 0xff),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.store8(0x5001, 0x180), Ok(StoreCompletion::Complete));
    byte_store_helper_only(&engine, &before, 3, [0, 0, 0, 0, 0, 1]);
}

#[test]
fn byte_store_terminal_latch_and_closed_guard_preserve_retained_resources_before_close() {
    let (mut engine, outer, home) = byte_store_fixture();
    let identity = engine.memory().unwrap().identity();
    let snapshots = [0x1000, 0x3000, 0x5000].map(|address| {
        engine
            .memory()
            .unwrap()
            .snapshot_code(GuestAddress(address), 2)
            .unwrap()
    });
    let artifact = engine
        .artifact
        .as_ref()
        .unwrap()
        .wasm_bytes(engine.memory().unwrap())
        .unwrap()
        .to_vec();
    let resident = engine.resident_bytes(home).unwrap().to_vec();
    let generation = engine.generation();
    engine.image_started = true;
    engine.exit_code = Some(0xf123_4567);
    engine.arena.as_mut().get_mut()[..100].fill(0xa5);
    let before = engine.arena().to_vec();
    assert_eq!(engine.write8(0x5000, 0xff), Err(HostError::ProcessExited));
    assert_eq!(engine.store8(0x5000, 0xff), Err(HostError::ProcessExited));
    assert_eq!(
        engine.store_resident8(0, 0, 0x5000, 0xff),
        Err(HostError::ProcessExited)
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(byte_store_ram(&engine, 0x5000, 4), [0x11, 0x22, 0x33, 0x44]);
    let memory = engine.memory.as_ref().unwrap();
    assert_eq!(memory.identity(), identity);
    assert!(
        snapshots
            .iter()
            .all(|snapshot| memory.is_code_current(snapshot))
    );
    assert_eq!(
        engine
            .artifact
            .as_ref()
            .unwrap()
            .wasm_bytes(memory)
            .unwrap(),
        artifact
    );
    assert_eq!(
        engine
            .resident
            .as_ref()
            .unwrap()
            .get_raw(memory, home)
            .unwrap()
            .wasm_bytes(memory)
            .unwrap(),
        resident
    );
    assert!(
        engine
            .resident
            .as_ref()
            .unwrap()
            .get_raw(memory, outer)
            .is_ok()
    );
    assert_eq!(engine.generation(), generation);
    assert_eq!(engine.exit_code, Some(0xf123_4567));
    assert!(engine.image_started);
    engine.read8(0x5000).unwrap();
    byte_store_helper_only(&engine, &before, 2, [0, 0x11, 0, 0, 0, 1]);
    let before_close = engine.arena().to_vec();
    engine.close();
    assert_eq!(engine.write8(0x5000, 0xff), Err(HostError::Closed));
    assert_eq!(engine.store8(0x5000, 0xff), Err(HostError::Closed));
    assert_eq!(
        engine.store_resident8(0, 0, 0x5000, 0xff),
        Err(HostError::Closed)
    );
    assert_eq!(engine.arena(), before_close);
    assert!(engine.memory.is_none());
    assert!(engine.artifact.is_none());
    assert!(engine.resident.is_none());
}

#[test]
fn word_store_cross_page_write_uses_one_content_version() {
    let (mut engine, outer, home) = byte_store_fixture();
    engine.unmap(0x8000, 1).unwrap();
    engine.map(0x6000, 1, 7).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x5ffe), &[0x11, 0x22, 0x33, 0x44])
        .unwrap();
    for resident in [false, true] {
        let first = engine
            .memory()
            .unwrap()
            .snapshot_code(GuestAddress(0x5ffe), 4)
            .unwrap();
        let before = engine.arena().to_vec();
        assert_eq!(
            if resident {
                engine.store_resident16(engine.key, home, 0x5fff, 0x9081)
            } else {
                engine.store16(0x5fff, 0x9081)
            },
            Ok(StoreCompletion::Complete)
        );
        byte_store_helper_only(&engine, &before, 4, [0, 0, 0, 0, 0, 2]);
        assert_eq!(byte_store_ram(&engine, 0x5ffe, 4), [0x11, 0x81, 0x90, 0x44]);
        let after = engine
            .memory()
            .unwrap()
            .snapshot_code(GuestAddress(0x5ffe), 4)
            .unwrap();
        assert_eq!(after.versions[0].content, after.versions[1].content);
        assert!(after.versions[0].content > first.versions[0].content);
        assert!(after.versions[1].content > first.versions[1].content);
        assert!(!engine.memory().unwrap().is_code_current(&first));
        engine.guard(engine.key, engine.generation()).unwrap();
        engine.guard_resident(engine.key, outer).unwrap();
        engine.guard_resident(engine.key, home).unwrap();
    }
}

#[test]
fn word_store_version_exhaustion_preserves_ram_versions_and_all_owners() {
    let (mut engine, outer, home) = byte_store_fixture();
    engine.unmap(0x8000, 1).unwrap();
    engine.map(0x6000, 1, 7).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x5ffe), &[0x11, 0x22, 0x33, 0x44])
        .unwrap();
    let artifact = engine.artifact_bytes().unwrap().to_vec();
    let resident = engine.resident_bytes(home).unwrap().to_vec();
    let artifact_pointer = engine.artifact_bytes().unwrap().as_ptr();
    let resident_pointer = engine.resident_bytes(home).unwrap().as_ptr();
    let snapshots = [(0x1000, 2), (0x3000, 3), (0x5ffe, 4)].map(|(address, length)| {
        engine
            .memory()
            .unwrap()
            .snapshot_code(GuestAddress(address), length)
            .unwrap()
    });
    engine.memory.as_mut().unwrap().exhaust_versions_for_test();
    for address in [0x5fff, 0x1000, 0x3000] {
        for use_resident in [false, true] {
            let before = engine.arena().to_vec();
            assert_eq!(
                if use_resident {
                    engine.store_resident16(engine.key, home, address, u32::MAX)
                } else {
                    engine.store16(address, u32::MAX)
                },
                Ok(StoreCompletion::Complete)
            );
            byte_store_helper_only(&engine, &before, 4, [2, 0, 1, 0, 0, 2]);
        }
    }
    assert_eq!(byte_store_ram(&engine, 0x1000, 2), [0x0f, 0x0b]);
    assert_eq!(byte_store_ram(&engine, 0x3000, 3), [0x90, 0xeb, 0]);
    assert_eq!(byte_store_ram(&engine, 0x5ffe, 4), [0x11, 0x22, 0x33, 0x44]);
    assert!(
        snapshots
            .iter()
            .all(|snapshot| engine.memory().unwrap().is_code_current(snapshot))
    );
    assert_eq!(engine.artifact_bytes().unwrap(), artifact);
    assert_eq!(engine.resident_bytes(home).unwrap(), resident);
    assert_eq!(engine.artifact_bytes().unwrap().as_ptr(), artifact_pointer);
    assert_eq!(
        engine.resident_bytes(home).unwrap().as_ptr(),
        resident_pointer
    );
    engine.guard(engine.key, engine.generation()).unwrap();
    engine.guard_resident(engine.key, outer).unwrap();
    engine.guard_resident(engine.key, home).unwrap();
}

#[test]
fn word_store_busy_terminal_and_closed_guards_leave_helper_and_ram_untouched() {
    for resident in [false, true] {
        let (mut engine, outer, home) = byte_store_fixture();
        byte_store_inject_gate(&mut engine);
        if resident {
            engine
                .capture_resident_call(
                    engine.key,
                    outer,
                    crate::windows::CallingConvention32::Cdecl,
                    1,
                )
                .unwrap();
        } else {
            engine
                .capture_call(
                    engine.key,
                    engine.generation(),
                    crate::windows::CallingConvention32::Cdecl,
                    1,
                )
                .unwrap();
        }
        let before = engine.arena().to_vec();
        for address in [0x5001, u32::MAX] {
            assert_eq!(
                engine.store16(address, u32::MAX),
                Err(HostError::Call(CallError::Busy))
            );
            assert_eq!(
                engine.store_resident16(engine.key, home, address, u32::MAX),
                Err(HostError::Call(CallError::Busy))
            );
            assert_eq!(engine.arena(), before);
            assert_eq!(byte_store_ram(&engine, 0x5000, 4), [0x11, 0x22, 0x33, 0x44]);
        }
        engine.exit_code = Some(7);
        assert_eq!(
            engine.store16(0x5001, u32::MAX),
            Err(HostError::ProcessExited)
        );
        assert_eq!(
            engine.store_resident16(0, 0, 0x5001, u32::MAX),
            Err(HostError::ProcessExited)
        );
        assert_eq!(engine.arena(), before);
        assert_eq!(byte_store_ram(&engine, 0x5000, 4), [0x11, 0x22, 0x33, 0x44]);
        engine.close();
        assert_eq!(engine.store16(0x5001, u32::MAX), Err(HostError::Closed));
        assert_eq!(
            engine.store_resident16(0, 0, 0x5001, u32::MAX),
            Err(HostError::Closed)
        );
        assert_eq!(engine.arena(), before);
    }
}

#[test]
fn word_store_callback_requires_active_resident_and_current_outer_authority() {
    use crate::process::calls::callback::{SuspendedCallback, SuspendedRecord};
    let (mut engine, outer_id, home_id) = byte_store_fixture();
    byte_store_inject_gate(&mut engine);
    engine
        .capture_resident_call(
            engine.key,
            outer_id,
            crate::windows::CallingConvention32::Cdecl,
            1,
        )
        .unwrap();
    let outer = engine.pending_call.take().unwrap();
    let token = outer.token + 1;
    engine.callback = Some(SuspendedCallback {
        parent: None,
        outer,
        record: SuspendedRecord::Resident {
            record: crate::abi::resident_callback::ResidentCallbackRecord32 {
                token,
                outer_token: token - 1,
                phase: 1,
                outcome: 0,
                entry_pc: 0x3000,
                entry_esp: 0x8000,
                return_pc: 0x1000,
                return_id: 8,
                stack_words: 0,
                result: 0,
                outer_unit_id: outer_id,
                callback_unit_id: home_id,
            },
            authorized: false,
            active_unit_id: home_id,
        },
    });
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.store16(0x5001, 0x9081),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.store_resident16(engine.key, home_id, 0x5001, 0x9081),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    if let SuspendedRecord::Resident { authorized, .. } =
        &mut engine.callback.as_mut().unwrap().record
    {
        *authorized = true;
    }
    assert_eq!(
        engine.store_resident16(engine.key, outer_id, 0x5001, 0x9081),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.store16(0x5001, 0x9081),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(
        engine.store_resident16(engine.key, home_id, 0x5001, 0x9081),
        Ok(StoreCompletion::Complete)
    );
    byte_store_helper_only(&engine, &before, 4, [0, 0, 0, 0, 0, 2]);
    assert_eq!(byte_store_ram(&engine, 0x5000, 4), [0x11, 0x81, 0x90, 0x44]);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1000), &[0x0f])
        .unwrap();
    let before_stale = engine.arena().to_vec();
    assert_eq!(
        engine.store_resident16(engine.key, home_id, 0x5001, 0xffff),
        Err(HostError::Resident(RegistryError::CodeInvalidated))
    );
    assert_eq!(engine.arena(), before_stale);
    assert_eq!(byte_store_ram(&engine, 0x5000, 4), [0x11, 0x81, 0x90, 0x44]);
}

#[test]
fn byte_exchange_exhaustion_preserves_ram_snapshots_and_all_published_owners() {
    for resident in [false, true] {
        let (mut engine, outer, home) = byte_store_fixture();
        engine.unmap(0x8000, 1).unwrap();
        engine.map(0xffff_f000, 1, 7).unwrap();
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(u32::MAX), &[0x5a])
            .unwrap();
        let bases = [0x1000, 0x3000, 0x5000, 0xffff_f000];
        let ram = bases.map(|base| byte_store_ram(&engine, base, 4096));
        let snapshots = bases.map(|base| {
            engine
                .memory()
                .unwrap()
                .snapshot_code(GuestAddress(base), 4096)
                .unwrap()
        });
        let generation = engine.generation();
        let modules = [
            engine.artifact_bytes().unwrap(),
            engine.resident_bytes(outer).unwrap(),
            engine.resident_bytes(home).unwrap(),
        ]
        .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize));
        let epoch = engine.memory().unwrap().code_validation_version();
        engine.memory.as_mut().unwrap().exhaust_versions_for_test();
        for address in [0x1000, 0x5001, u32::MAX] {
            for _ in 0..2 {
                let before = engine.arena().to_vec();
                assert_eq!(
                    if resident {
                        engine.exchange_resident8(engine.key, home, address, u32::MAX)
                    } else {
                        engine.exchange8(address, u32::MAX)
                    },
                    Ok(StoreCompletion::Complete)
                );
                byte_store_helper_only(&engine, &before, 5, [2, 0, 1, 0, 0, 1]);
                assert_eq!(engine.memory().unwrap().code_validation_version(), epoch);
                assert_eq!(engine.generation(), generation);
                for (index, bytes) in [
                    engine.artifact_bytes().unwrap(),
                    engine.resident_bytes(outer).unwrap(),
                    engine.resident_bytes(home).unwrap(),
                ]
                .into_iter()
                .enumerate()
                {
                    assert_eq!(bytes, modules[index].0);
                    assert_eq!(bytes.as_ptr() as usize, modules[index].1);
                }
                for (index, base) in bases.into_iter().enumerate() {
                    assert_eq!(byte_store_ram(&engine, base, 4096), ram[index]);
                    assert!(engine.memory().unwrap().is_code_current(&snapshots[index]));
                }
                engine.guard(engine.key, generation).unwrap();
                engine.guard_resident(engine.key, outer).unwrap();
                engine.guard_resident(engine.key, home).unwrap();
            }
        }
    }
}

#[test]
fn byte_exchange_callback_requires_the_active_resident_and_current_outer_owner() {
    use crate::process::calls::callback::{SuspendedCallback, SuspendedRecord};
    let (mut engine, outer_id, home_id) = byte_store_fixture();
    byte_store_inject_gate(&mut engine);
    engine
        .capture_resident_call(
            engine.key,
            outer_id,
            crate::windows::CallingConvention32::Cdecl,
            1,
        )
        .unwrap();
    let outer = engine.pending_call.take().unwrap();
    let token = outer.token + 1;
    engine.callback = Some(SuspendedCallback {
        parent: None,
        outer,
        record: SuspendedRecord::Resident {
            record: crate::abi::resident_callback::ResidentCallbackRecord32 {
                token,
                outer_token: token - 1,
                phase: 1,
                outcome: 0,
                entry_pc: 0x3000,
                entry_esp: 0x8000,
                return_pc: 0x1000,
                return_id: 8,
                stack_words: 0,
                result: 0,
                outer_unit_id: outer_id,
                callback_unit_id: home_id,
            },
            authorized: false,
            active_unit_id: home_id,
        },
    });
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.exchange8(0x5001, 0x81),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.exchange_resident8(engine.key, home_id, 0x5001, 0x81),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    if let SuspendedRecord::Resident { authorized, .. } =
        &mut engine.callback.as_mut().unwrap().record
    {
        *authorized = true;
    }
    assert_eq!(
        engine.exchange_resident8(engine.key, outer_id, 0x5001, 0x81),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.exchange8(0x5001, 0x81),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(
        engine.exchange_resident8(engine.key, home_id, 0x5001, 0xffff_ff81),
        Ok(StoreCompletion::Complete)
    );
    byte_store_helper_only(&engine, &before, 5, [0, 0x22, 0, 0, 0, 1]);
    assert_eq!(byte_store_ram(&engine, 0x5000, 4), [0x11, 0x81, 0x33, 0x44]);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1000), &[0x0f])
        .unwrap();
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.exchange_resident8(engine.key, home_id, 0x5001, 0xff),
        Err(HostError::Resident(RegistryError::CodeInvalidated))
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(byte_store_ram(&engine, 0x5000, 4), [0x11, 0x81, 0x33, 0x44]);
}

#[test]
fn word_exchange_exhaustion_preserves_ram_snapshots_and_all_published_owners() {
    for resident in [false, true] {
        let (mut engine, outer, home) = byte_store_fixture();
        engine.unmap(0x8000, 1).unwrap();
        engine.map(0xffff_f000, 1, 7).unwrap();
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(u32::MAX - 1), &[0x5a, 0xa5])
            .unwrap();
        let bases = [0x1000, 0x3000, 0x5000, 0xffff_f000];
        let ram = bases.map(|base| byte_store_ram(&engine, base, 4096));
        let snapshots = bases.map(|base| {
            engine
                .memory()
                .unwrap()
                .snapshot_code(GuestAddress(base), 4096)
                .unwrap()
        });
        let generation = engine.generation();
        let modules = [
            engine.artifact_bytes().unwrap(),
            engine.resident_bytes(outer).unwrap(),
            engine.resident_bytes(home).unwrap(),
        ]
        .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize));
        let epoch = engine.memory().unwrap().code_validation_version();
        engine.memory.as_mut().unwrap().exhaust_versions_for_test();
        for address in [0x1000, 0x5001, u32::MAX - 1] {
            for _ in 0..2 {
                let before = engine.arena().to_vec();
                assert_eq!(
                    if resident {
                        engine.exchange_resident16(engine.key, home, address, u32::MAX)
                    } else {
                        engine.exchange16(address, u32::MAX)
                    },
                    Ok(StoreCompletion::Complete)
                );
                byte_store_helper_only(&engine, &before, 6, [2, 0, 1, 0, 0, 2]);
                assert_eq!(engine.memory().unwrap().code_validation_version(), epoch);
                assert_eq!(engine.generation(), generation);
                for (index, bytes) in [
                    engine.artifact_bytes().unwrap(),
                    engine.resident_bytes(outer).unwrap(),
                    engine.resident_bytes(home).unwrap(),
                ]
                .into_iter()
                .enumerate()
                {
                    assert_eq!(bytes, modules[index].0);
                    assert_eq!(bytes.as_ptr() as usize, modules[index].1);
                }
                for (index, base) in bases.into_iter().enumerate() {
                    assert_eq!(byte_store_ram(&engine, base, 4096), ram[index]);
                    assert!(engine.memory().unwrap().is_code_current(&snapshots[index]));
                }
                engine.guard(engine.key, generation).unwrap();
                engine.guard_resident(engine.key, outer).unwrap();
                engine.guard_resident(engine.key, home).unwrap();
            }
        }
    }
}

#[test]
fn word_exchange_callback_requires_the_active_resident_and_current_outer_owner() {
    use crate::process::calls::callback::{SuspendedCallback, SuspendedRecord};
    let (mut engine, outer_id, home_id) = byte_store_fixture();
    byte_store_inject_gate(&mut engine);
    engine
        .capture_resident_call(
            engine.key,
            outer_id,
            crate::windows::CallingConvention32::Cdecl,
            1,
        )
        .unwrap();
    let outer = engine.pending_call.take().unwrap();
    let token = outer.token + 1;
    engine.callback = Some(SuspendedCallback {
        parent: None,
        outer,
        record: SuspendedRecord::Resident {
            record: crate::abi::resident_callback::ResidentCallbackRecord32 {
                token,
                outer_token: token - 1,
                phase: 1,
                outcome: 0,
                entry_pc: 0x3000,
                entry_esp: 0x8000,
                return_pc: 0x1000,
                return_id: 8,
                stack_words: 0,
                result: 0,
                outer_unit_id: outer_id,
                callback_unit_id: home_id,
            },
            authorized: false,
            active_unit_id: home_id,
        },
    });
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.exchange16(0x5001, 0x9081),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.exchange_resident16(engine.key, home_id, 0x5001, 0x9081),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    if let SuspendedRecord::Resident { authorized, .. } =
        &mut engine.callback.as_mut().unwrap().record
    {
        *authorized = true;
    }
    assert_eq!(
        engine.exchange_resident16(engine.key, outer_id, 0x5001, 0x9081),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.exchange16(0x5001, 0x9081),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(
        engine.exchange_resident16(engine.key, home_id, 0x5001, 0xffff_9081),
        Ok(StoreCompletion::Complete)
    );
    byte_store_helper_only(&engine, &before, 6, [0, 0x3322, 0, 0, 0, 2]);
    assert_eq!(byte_store_ram(&engine, 0x5000, 4), [0x11, 0x81, 0x90, 0x44]);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1000), &[0x0f])
        .unwrap();
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.exchange_resident16(engine.key, home_id, 0x5001, 0xff),
        Err(HostError::Resident(RegistryError::CodeInvalidated))
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(byte_store_ram(&engine, 0x5000, 4), [0x11, 0x81, 0x90, 0x44]);
}

#[test]
fn word_exchange_busy_terminal_and_closed_guards_leave_helper_and_ram_untouched() {
    for resident in [false, true] {
        let (mut engine, outer, home) = byte_store_fixture();
        byte_store_inject_gate(&mut engine);
        if resident {
            engine
                .capture_resident_call(
                    engine.key,
                    outer,
                    crate::windows::CallingConvention32::Cdecl,
                    1,
                )
                .unwrap();
        } else {
            engine
                .capture_call(
                    engine.key,
                    engine.generation(),
                    crate::windows::CallingConvention32::Cdecl,
                    1,
                )
                .unwrap();
        }
        let before = engine.arena().to_vec();
        for address in [0x5001, u32::MAX] {
            assert_eq!(
                engine.exchange16(address, u32::MAX),
                Err(HostError::Call(CallError::Busy))
            );
            assert_eq!(
                engine.exchange_resident16(engine.key, home, address, u32::MAX),
                Err(HostError::Call(CallError::Busy))
            );
            assert_eq!(engine.arena(), before);
            assert_eq!(byte_store_ram(&engine, 0x5000, 4), [0x11, 0x22, 0x33, 0x44]);
        }
        engine.exit_code = Some(7);
        assert_eq!(
            engine.exchange16(0x5001, u32::MAX),
            Err(HostError::ProcessExited)
        );
        assert_eq!(
            engine.exchange_resident16(0, 0, 0x5001, u32::MAX),
            Err(HostError::ProcessExited)
        );
        assert_eq!(engine.arena(), before);
        assert_eq!(byte_store_ram(&engine, 0x5000, 4), [0x11, 0x22, 0x33, 0x44]);
        engine.close();
        assert_eq!(engine.exchange16(0x5001, u32::MAX), Err(HostError::Closed));
        assert_eq!(
            engine.exchange_resident16(0, 0, 0x5001, u32::MAX),
            Err(HostError::Closed)
        );
        assert_eq!(engine.arena(), before);
    }
}

#[test]
fn compare_exchange_exhaustion_preserves_every_published_owner_and_snapshot() {
    for resident in [false, true] {
        let (mut engine, outer, home) = byte_store_fixture();
        engine.unmap(0x8000, 1).unwrap();
        engine.map(0xffff_f000, 1, 7).unwrap();
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(0xffff_fffc), &0x8765_4321_u32.to_le_bytes())
            .unwrap();
        let bases = [0x1000, 0x3000, 0x5000, 0xffff_f000];
        let ram = bases.map(|base| byte_store_ram(&engine, base, 4096));
        let snapshots = bases.map(|base| {
            engine
                .memory()
                .unwrap()
                .snapshot_code(GuestAddress(base), 4096)
                .unwrap()
        });
        let modules = [
            engine.artifact_bytes().unwrap(),
            engine.resident_bytes(outer).unwrap(),
            engine.resident_bytes(home).unwrap(),
        ]
        .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize));
        let generation = engine.generation();
        let identity = engine.memory().unwrap().identity();
        let epoch = engine.memory().unwrap().code_validation_version();
        engine.memory.as_mut().unwrap().exhaust_versions_for_test();
        for address in [0x1000, 0x5000, 0xffff_fffc] {
            let old = u32::from_le_bytes(byte_store_ram(&engine, address, 4).try_into().unwrap());
            for expected in [old, old ^ 1] {
                let before = engine.arena().to_vec();
                assert_eq!(
                    if resident {
                        engine.compare_exchange_resident32(
                            engine.key,
                            home,
                            address,
                            expected,
                            0xdead_beef,
                        )
                    } else {
                        engine.compare_exchange32(address, expected, 0xdead_beef)
                    },
                    Ok(StoreCompletion::Complete)
                );
                byte_store_helper_only(&engine, &before, 1, [2, 0, 1, 0, 0, 0]);
                assert_eq!(engine.generation(), generation);
                assert_eq!(engine.memory().unwrap().identity(), identity);
                assert_eq!(engine.memory().unwrap().code_validation_version(), epoch);
                for (index, bytes) in [
                    engine.artifact_bytes().unwrap(),
                    engine.resident_bytes(outer).unwrap(),
                    engine.resident_bytes(home).unwrap(),
                ]
                .into_iter()
                .enumerate()
                {
                    assert_eq!(bytes, modules[index].0);
                    assert_eq!(bytes.as_ptr() as usize, modules[index].1);
                }
                for (index, base) in bases.into_iter().enumerate() {
                    assert_eq!(byte_store_ram(&engine, base, 4096), ram[index]);
                    assert!(engine.memory().unwrap().is_code_current(&snapshots[index]));
                }
                engine.guard(engine.key, generation).unwrap();
                engine.guard_resident(engine.key, outer).unwrap();
                engine.guard_resident(engine.key, home).unwrap();
            }
        }
    }
}

#[test]
fn compare_exchange_callback_authority_and_lifecycle_precede_all_effects() {
    use crate::process::calls::callback::{SuspendedCallback, SuspendedRecord};
    let (mut engine, outer_id, home_id) = byte_store_fixture();
    byte_store_inject_gate(&mut engine);
    engine
        .capture_resident_call(
            engine.key,
            outer_id,
            crate::windows::CallingConvention32::Cdecl,
            1,
        )
        .unwrap();
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.compare_exchange32(u32::MAX, 0, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.compare_exchange_resident32(engine.key, home_id, u32::MAX, 0, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    let outer = engine.pending_call.take().unwrap();
    let token = outer.token + 1;
    engine.callback = Some(SuspendedCallback {
        parent: None,
        outer,
        record: SuspendedRecord::Resident {
            record: crate::abi::resident_callback::ResidentCallbackRecord32 {
                token,
                outer_token: token - 1,
                phase: 1,
                outcome: 0,
                entry_pc: 0x3000,
                entry_esp: 0x8000,
                return_pc: 0x1000,
                return_id: 8,
                stack_words: 0,
                result: 0,
                outer_unit_id: outer_id,
                callback_unit_id: home_id,
            },
            authorized: false,
            active_unit_id: home_id,
        },
    });
    assert_eq!(
        engine.compare_exchange32(0x5000, 0, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.compare_exchange_resident32(engine.key, home_id, 0x5000, 0, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    if let SuspendedRecord::Resident { authorized, .. } =
        &mut engine.callback.as_mut().unwrap().record
    {
        *authorized = true;
    }
    assert_eq!(
        engine.compare_exchange_resident32(engine.key, outer_id, 0x5000, 0, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.compare_exchange32(0x5000, 0, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    for (expected, desired, committed) in [
        (0x4433_2210, 0xdead_beef, 0x4433_2211_u32),
        (0x4433_2211, 0x8877_6655, 0x8877_6655),
    ] {
        let before = engine.arena().to_vec();
        assert_eq!(
            engine.compare_exchange_resident32(engine.key, home_id, 0x5000, expected, desired),
            Ok(StoreCompletion::Complete)
        );
        byte_store_helper_only(&engine, &before, 1, [0, 0x4433_2211, 0, 0, 0, 0]);
        assert_eq!(byte_store_ram(&engine, 0x5000, 4), committed.to_le_bytes());
    }
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1000), &[0x0f])
        .unwrap();
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.compare_exchange_resident32(engine.key, home_id, 0x5000, 0x8877_6655, 0),
        Err(HostError::Resident(RegistryError::CodeInvalidated))
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(byte_store_ram(&engine, 0x5000, 4), [0x55, 0x66, 0x77, 0x88]);
    engine.exit_code = Some(7);
    assert_eq!(
        engine.compare_exchange32(u32::MAX, 0, 1),
        Err(HostError::ProcessExited)
    );
    assert_eq!(
        engine.compare_exchange_resident32(0, 0, u32::MAX, 0, 1),
        Err(HostError::ProcessExited)
    );
    assert_eq!(engine.arena(), before);
    engine.close();
    assert_eq!(
        engine.compare_exchange32(u32::MAX, 0, 1),
        Err(HostError::Closed)
    );
    assert_eq!(
        engine.compare_exchange_resident32(0, 0, u32::MAX, 0, 1),
        Err(HostError::Closed)
    );
    assert_eq!(engine.arena(), before);
}

#[test]
fn exchange_add_exhaustion_preserves_every_published_owner_and_snapshot() {
    for resident in [false, true] {
        let (mut engine, outer, home) = byte_store_fixture();
        engine.unmap(0x8000, 1).unwrap();
        engine.map(0xffff_f000, 1, 7).unwrap();
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(0xffff_fffc), &0x8765_4321_u32.to_le_bytes())
            .unwrap();
        let bases = [0x1000, 0x3000, 0x5000, 0xffff_f000];
        let ram = bases.map(|base| byte_store_ram(&engine, base, 4096));
        let snapshots = bases.map(|base| {
            engine
                .memory()
                .unwrap()
                .snapshot_code(GuestAddress(base), 4096)
                .unwrap()
        });
        let modules = [
            engine.artifact_bytes().unwrap(),
            engine.resident_bytes(outer).unwrap(),
            engine.resident_bytes(home).unwrap(),
        ]
        .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize));
        let generation = engine.generation();
        let identity = engine.memory().unwrap().identity();
        let epoch = engine.memory().unwrap().code_validation_version();
        engine.memory.as_mut().unwrap().exhaust_versions_for_test();
        for address in [0x1000, 0x5000, 0xffff_fffc] {
            for source in [0, 1] {
                let before = engine.arena().to_vec();
                assert_eq!(
                    if resident {
                        engine.exchange_add_resident32(engine.key, home, address, source)
                    } else {
                        engine.exchange_add32(address, source)
                    },
                    Ok(StoreCompletion::Complete)
                );
                byte_store_helper_only(&engine, &before, 1, [2, 0, 1, 0, 0, 0]);
                assert_eq!(engine.generation(), generation);
                assert_eq!(engine.memory().unwrap().identity(), identity);
                assert_eq!(engine.memory().unwrap().code_validation_version(), epoch);
                for (index, bytes) in [
                    engine.artifact_bytes().unwrap(),
                    engine.resident_bytes(outer).unwrap(),
                    engine.resident_bytes(home).unwrap(),
                ]
                .into_iter()
                .enumerate()
                {
                    assert_eq!(bytes, modules[index].0);
                    assert_eq!(bytes.as_ptr() as usize, modules[index].1);
                }
                for (index, base) in bases.into_iter().enumerate() {
                    assert_eq!(byte_store_ram(&engine, base, 4096), ram[index]);
                    assert!(engine.memory().unwrap().is_code_current(&snapshots[index]));
                }
                engine.guard(engine.key, generation).unwrap();
                engine.guard_resident(engine.key, outer).unwrap();
                engine.guard_resident(engine.key, home).unwrap();
            }
        }
    }
}

#[test]
fn exchange_add_callback_authority_and_lifecycle_precede_all_effects() {
    use crate::process::calls::callback::{SuspendedCallback, SuspendedRecord};
    let (mut engine, outer_id, home_id) = byte_store_fixture();
    byte_store_inject_gate(&mut engine);
    engine
        .capture_resident_call(
            engine.key,
            outer_id,
            crate::windows::CallingConvention32::Cdecl,
            1,
        )
        .unwrap();
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.exchange_add32(u32::MAX, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.exchange_add_resident32(engine.key, home_id, u32::MAX, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    let outer = engine.pending_call.take().unwrap();
    let token = outer.token + 1;
    engine.callback = Some(SuspendedCallback {
        parent: None,
        outer,
        record: SuspendedRecord::Resident {
            record: crate::abi::resident_callback::ResidentCallbackRecord32 {
                token,
                outer_token: token - 1,
                phase: 1,
                outcome: 0,
                entry_pc: 0x3000,
                entry_esp: 0x8000,
                return_pc: 0x1000,
                return_id: 8,
                stack_words: 0,
                result: 0,
                outer_unit_id: outer_id,
                callback_unit_id: home_id,
            },
            authorized: false,
            active_unit_id: home_id,
        },
    });
    assert_eq!(
        engine.exchange_add32(0x5000, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.exchange_add_resident32(engine.key, home_id, 0x5000, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    if let SuspendedRecord::Resident { authorized, .. } =
        &mut engine.callback.as_mut().unwrap().record
    {
        *authorized = true;
    }
    assert_eq!(
        engine.exchange_add_resident32(engine.key, outer_id, 0x5000, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.exchange_add32(0x5000, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    for (source, committed) in [(0, 0x4433_2211_u32), (0x4444_4444, 0x8877_6655)] {
        let before = engine.arena().to_vec();
        assert_eq!(
            engine.exchange_add_resident32(engine.key, home_id, 0x5000, source),
            Ok(StoreCompletion::Complete)
        );
        byte_store_helper_only(&engine, &before, 1, [0, 0x4433_2211, 0, 0, 0, 0]);
        assert_eq!(byte_store_ram(&engine, 0x5000, 4), committed.to_le_bytes());
    }
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1000), &[0x0f])
        .unwrap();
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.exchange_add_resident32(engine.key, home_id, 0x5000, 0),
        Err(HostError::Resident(RegistryError::CodeInvalidated))
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(byte_store_ram(&engine, 0x5000, 4), [0x55, 0x66, 0x77, 0x88]);
    engine.exit_code = Some(7);
    assert_eq!(
        engine.exchange_add32(u32::MAX, 1),
        Err(HostError::ProcessExited)
    );
    assert_eq!(
        engine.exchange_add_resident32(0, 0, u32::MAX, 1),
        Err(HostError::ProcessExited)
    );
    assert_eq!(engine.arena(), before);
    engine.close();
    assert_eq!(engine.exchange_add32(u32::MAX, 1), Err(HostError::Closed));
    assert_eq!(
        engine.exchange_add_resident32(0, 0, u32::MAX, 1),
        Err(HostError::Closed)
    );
    assert_eq!(engine.arena(), before);
}
#[test]
fn byte_compare_exchange_exhaustion_preserves_every_published_owner_and_snapshot() {
    for resident in [false, true] {
        let (mut engine, outer, home) = byte_store_fixture();
        engine.unmap(0x8000, 1).unwrap();
        engine.map(0xffff_f000, 1, 7).unwrap();
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(u32::MAX), &[0x5a])
            .unwrap();
        let bases = [0x1000, 0x3000, 0x5000, 0xffff_f000];
        let ram = bases.map(|base| byte_store_ram(&engine, base, 4096));
        let snapshots = bases.map(|base| {
            engine
                .memory()
                .unwrap()
                .snapshot_code(GuestAddress(base), 4096)
                .unwrap()
        });
        let modules = [
            engine.artifact_bytes().unwrap(),
            engine.resident_bytes(outer).unwrap(),
            engine.resident_bytes(home).unwrap(),
        ]
        .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize));
        let generation = engine.generation();
        let identity = engine.memory().unwrap().identity();
        let epoch = engine.memory().unwrap().code_validation_version();
        engine.memory.as_mut().unwrap().exhaust_versions_for_test();
        for address in [0x1000, 0x5001, u32::MAX] {
            let old = u32::from(byte_store_ram(&engine, address, 1)[0]);
            for expected in [old, old ^ 1] {
                let before = engine.arena().to_vec();
                assert_eq!(
                    if resident {
                        engine.compare_exchange_resident8(
                            engine.key,
                            home,
                            address,
                            expected,
                            0xdead_beef,
                        )
                    } else {
                        engine.compare_exchange8(address, expected, 0xdead_beef)
                    },
                    Ok(StoreCompletion::Complete)
                );
                byte_store_helper_only(&engine, &before, 5, [2, 0, 1, 0, 0, 1]);
                assert_eq!(engine.generation(), generation);
                assert_eq!(engine.memory().unwrap().identity(), identity);
                assert_eq!(engine.memory().unwrap().code_validation_version(), epoch);
                for (index, bytes) in [
                    engine.artifact_bytes().unwrap(),
                    engine.resident_bytes(outer).unwrap(),
                    engine.resident_bytes(home).unwrap(),
                ]
                .into_iter()
                .enumerate()
                {
                    assert_eq!(bytes, modules[index].0);
                    assert_eq!(bytes.as_ptr() as usize, modules[index].1);
                }
                for (index, base) in bases.into_iter().enumerate() {
                    assert_eq!(byte_store_ram(&engine, base, 4096), ram[index]);
                    assert!(engine.memory().unwrap().is_code_current(&snapshots[index]));
                }
                engine.guard(engine.key, generation).unwrap();
                engine.guard_resident(engine.key, outer).unwrap();
                engine.guard_resident(engine.key, home).unwrap();
            }
        }
    }
}

#[test]
fn byte_compare_exchange_callback_authority_and_lifecycle_precede_all_effects() {
    use crate::process::calls::callback::{SuspendedCallback, SuspendedRecord};
    let (mut engine, outer_id, home_id) = byte_store_fixture();
    byte_store_inject_gate(&mut engine);
    engine
        .capture_resident_call(
            engine.key,
            outer_id,
            crate::windows::CallingConvention32::Cdecl,
            1,
        )
        .unwrap();
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.compare_exchange8(u32::MAX, 0, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.compare_exchange_resident8(engine.key, home_id, u32::MAX, 0, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    let outer = engine.pending_call.take().unwrap();
    let token = outer.token + 1;
    engine.callback = Some(SuspendedCallback {
        parent: None,
        outer,
        record: SuspendedRecord::Resident {
            record: crate::abi::resident_callback::ResidentCallbackRecord32 {
                token,
                outer_token: token - 1,
                phase: 1,
                outcome: 0,
                entry_pc: 0x3000,
                entry_esp: 0x8000,
                return_pc: 0x1000,
                return_id: 8,
                stack_words: 0,
                result: 0,
                outer_unit_id: outer_id,
                callback_unit_id: home_id,
            },
            authorized: false,
            active_unit_id: home_id,
        },
    });
    assert_eq!(
        engine.compare_exchange8(0x5000, 0, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.compare_exchange_resident8(engine.key, home_id, 0x5000, 0, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    if let SuspendedRecord::Resident { authorized, .. } =
        &mut engine.callback.as_mut().unwrap().record
    {
        *authorized = true;
    }
    assert_eq!(
        engine.compare_exchange_resident8(engine.key, outer_id, 0x5000, 0, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.compare_exchange8(0x5000, 0, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    for (expected, desired, committed) in [
        (0xffff_ff21, 0xdead_beef, 0x22_u8),
        (0xffff_ff22, 0xffff_ff81, 0x81),
    ] {
        let before = engine.arena().to_vec();
        assert_eq!(
            engine.compare_exchange_resident8(engine.key, home_id, 0x5001, expected, desired),
            Ok(StoreCompletion::Complete)
        );
        byte_store_helper_only(&engine, &before, 5, [0, 0x22, 0, 0, 0, 1]);
        assert_eq!(
            byte_store_ram(&engine, 0x5000, 4),
            [0x11, committed, 0x33, 0x44]
        );
    }
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1000), &[0x0f])
        .unwrap();
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.compare_exchange_resident8(engine.key, home_id, 0x5001, 0x81, 0),
        Err(HostError::Resident(RegistryError::CodeInvalidated))
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(byte_store_ram(&engine, 0x5000, 4), [0x11, 0x81, 0x33, 0x44]);
    engine.exit_code = Some(7);
    assert_eq!(
        engine.compare_exchange8(u32::MAX, 0, 1),
        Err(HostError::ProcessExited)
    );
    assert_eq!(
        engine.compare_exchange_resident8(0, 0, u32::MAX, 0, 1),
        Err(HostError::ProcessExited)
    );
    assert_eq!(engine.arena(), before);
    engine.close();
    assert_eq!(
        engine.compare_exchange8(u32::MAX, 0, 1),
        Err(HostError::Closed)
    );
    assert_eq!(
        engine.compare_exchange_resident8(0, 0, u32::MAX, 0, 1),
        Err(HostError::Closed)
    );
    assert_eq!(engine.arena(), before);
}
#[test]
fn byte_exchange_add_exhaustion_preserves_every_published_owner_and_snapshot() {
    for resident in [false, true] {
        let (mut engine, outer, home) = byte_store_fixture();
        engine.unmap(0x8000, 1).unwrap();
        engine.map(0xffff_f000, 1, 7).unwrap();
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(u32::MAX), &[0x5a])
            .unwrap();
        let bases = [0x1000, 0x3000, 0x5000, 0xffff_f000];
        let ram = bases.map(|base| byte_store_ram(&engine, base, 4096));
        let snapshots = bases.map(|base| {
            engine
                .memory()
                .unwrap()
                .snapshot_code(GuestAddress(base), 4096)
                .unwrap()
        });
        let modules = [
            engine.artifact_bytes().unwrap(),
            engine.resident_bytes(outer).unwrap(),
            engine.resident_bytes(home).unwrap(),
        ]
        .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize));
        let generation = engine.generation();
        let identity = engine.memory().unwrap().identity();
        let epoch = engine.memory().unwrap().code_validation_version();
        engine.memory.as_mut().unwrap().exhaust_versions_for_test();
        for address in [0x1000, 0x5001, u32::MAX] {
            for source in [0, u32::MAX] {
                let before = engine.arena().to_vec();
                assert_eq!(
                    if resident {
                        engine.exchange_add_resident8(engine.key, home, address, source)
                    } else {
                        engine.exchange_add8(address, source)
                    },
                    Ok(StoreCompletion::Complete)
                );
                byte_store_helper_only(&engine, &before, 5, [2, 0, 1, 0, 0, 1]);
                assert_eq!(engine.generation(), generation);
                assert_eq!(engine.memory().unwrap().identity(), identity);
                assert_eq!(engine.memory().unwrap().code_validation_version(), epoch);
                for (index, bytes) in [
                    engine.artifact_bytes().unwrap(),
                    engine.resident_bytes(outer).unwrap(),
                    engine.resident_bytes(home).unwrap(),
                ]
                .into_iter()
                .enumerate()
                {
                    assert_eq!(bytes, modules[index].0);
                    assert_eq!(bytes.as_ptr() as usize, modules[index].1);
                }
                for (index, base) in bases.into_iter().enumerate() {
                    assert_eq!(byte_store_ram(&engine, base, 4096), ram[index]);
                    assert!(engine.memory().unwrap().is_code_current(&snapshots[index]));
                }
                engine.guard(engine.key, generation).unwrap();
                engine.guard_resident(engine.key, outer).unwrap();
                engine.guard_resident(engine.key, home).unwrap();
            }
        }
    }
}

#[test]
fn byte_exchange_add_callback_authority_and_lifecycle_precede_all_effects() {
    use crate::process::calls::callback::{SuspendedCallback, SuspendedRecord};
    let (mut engine, outer_id, home_id) = byte_store_fixture();
    byte_store_inject_gate(&mut engine);
    engine
        .capture_resident_call(
            engine.key,
            outer_id,
            crate::windows::CallingConvention32::Cdecl,
            1,
        )
        .unwrap();
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.exchange_add8(u32::MAX, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.exchange_add_resident8(engine.key, home_id, u32::MAX, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    let outer = engine.pending_call.take().unwrap();
    let token = outer.token + 1;
    engine.callback = Some(SuspendedCallback {
        parent: None,
        outer,
        record: SuspendedRecord::Resident {
            record: crate::abi::resident_callback::ResidentCallbackRecord32 {
                token,
                outer_token: token - 1,
                phase: 1,
                outcome: 0,
                entry_pc: 0x3000,
                entry_esp: 0x8000,
                return_pc: 0x1000,
                return_id: 8,
                stack_words: 0,
                result: 0,
                outer_unit_id: outer_id,
                callback_unit_id: home_id,
            },
            authorized: false,
            active_unit_id: home_id,
        },
    });
    assert_eq!(
        engine.exchange_add8(0x5000, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.exchange_add_resident8(engine.key, home_id, 0x5000, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    if let SuspendedRecord::Resident { authorized, .. } =
        &mut engine.callback.as_mut().unwrap().record
    {
        *authorized = true;
    }
    assert_eq!(
        engine.exchange_add_resident8(engine.key, outer_id, 0x5000, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.exchange_add8(0x5000, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    for (source, committed) in [(0xffff_ff00, 0x22_u8), (0xffff_ff5f, 0x81)] {
        let before = engine.arena().to_vec();
        assert_eq!(
            engine.exchange_add_resident8(engine.key, home_id, 0x5001, source),
            Ok(StoreCompletion::Complete)
        );
        byte_store_helper_only(&engine, &before, 5, [0, 0x22, 0, 0, 0, 1]);
        assert_eq!(
            byte_store_ram(&engine, 0x5000, 4),
            [0x11, committed, 0x33, 0x44]
        );
    }
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1000), &[0x0f])
        .unwrap();
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.exchange_add_resident8(engine.key, home_id, 0x5001, 0),
        Err(HostError::Resident(RegistryError::CodeInvalidated))
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(byte_store_ram(&engine, 0x5000, 4), [0x11, 0x81, 0x33, 0x44]);
    engine.exit_code = Some(7);
    assert_eq!(
        engine.exchange_add8(u32::MAX, 1),
        Err(HostError::ProcessExited)
    );
    assert_eq!(
        engine.exchange_add_resident8(0, 0, u32::MAX, 1),
        Err(HostError::ProcessExited)
    );
    assert_eq!(engine.arena(), before);
    engine.close();
    assert_eq!(engine.exchange_add8(u32::MAX, 1), Err(HostError::Closed));
    assert_eq!(
        engine.exchange_add_resident8(0, 0, u32::MAX, 1),
        Err(HostError::Closed)
    );
    assert_eq!(engine.arena(), before);
}
#[test]
fn word_compare_exchange_exhaustion_preserves_every_published_owner_and_snapshot() {
    for resident in [false, true] {
        let (mut engine, outer, home) = byte_store_fixture();
        engine.unmap(0x8000, 1).unwrap();
        engine.map(0xffff_f000, 1, 7).unwrap();
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(u32::MAX - 1), &[0x5a, 0xa5])
            .unwrap();
        let bases = [0x1000, 0x3000, 0x5000, 0xffff_f000];
        let ram = bases.map(|base| byte_store_ram(&engine, base, 4096));
        let snapshots = bases.map(|base| {
            engine
                .memory()
                .unwrap()
                .snapshot_code(GuestAddress(base), 4096)
                .unwrap()
        });
        let modules = [
            engine.artifact_bytes().unwrap(),
            engine.resident_bytes(outer).unwrap(),
            engine.resident_bytes(home).unwrap(),
        ]
        .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize));
        let generation = engine.generation();
        let identity = engine.memory().unwrap().identity();
        let epoch = engine.memory().unwrap().code_validation_version();
        engine.memory.as_mut().unwrap().exhaust_versions_for_test();
        for address in [0x1000, 0x5001, u32::MAX - 1] {
            let bytes = byte_store_ram(&engine, address, 2);
            let old = u32::from(u16::from_le_bytes([bytes[0], bytes[1]]));
            for expected in [old, old ^ 1] {
                let before = engine.arena().to_vec();
                assert_eq!(
                    if resident {
                        engine.compare_exchange_resident16(
                            engine.key,
                            home,
                            address,
                            expected,
                            0xdead_beef,
                        )
                    } else {
                        engine.compare_exchange16(address, expected, 0xdead_beef)
                    },
                    Ok(StoreCompletion::Complete)
                );
                byte_store_helper_only(&engine, &before, 6, [2, 0, 1, 0, 0, 2]);
                assert_eq!(engine.generation(), generation);
                assert_eq!(engine.memory().unwrap().identity(), identity);
                assert_eq!(engine.memory().unwrap().code_validation_version(), epoch);
                for (index, bytes) in [
                    engine.artifact_bytes().unwrap(),
                    engine.resident_bytes(outer).unwrap(),
                    engine.resident_bytes(home).unwrap(),
                ]
                .into_iter()
                .enumerate()
                {
                    assert_eq!(bytes, modules[index].0);
                    assert_eq!(bytes.as_ptr() as usize, modules[index].1);
                }
                for (index, base) in bases.into_iter().enumerate() {
                    assert_eq!(byte_store_ram(&engine, base, 4096), ram[index]);
                    assert!(engine.memory().unwrap().is_code_current(&snapshots[index]));
                }
                engine.guard(engine.key, generation).unwrap();
                engine.guard_resident(engine.key, outer).unwrap();
                engine.guard_resident(engine.key, home).unwrap();
            }
        }
    }
}

#[test]
fn word_compare_exchange_callback_authority_and_lifecycle_precede_all_effects() {
    use crate::process::calls::callback::{SuspendedCallback, SuspendedRecord};
    let (mut engine, outer_id, home_id) = byte_store_fixture();
    byte_store_inject_gate(&mut engine);
    engine
        .capture_resident_call(
            engine.key,
            outer_id,
            crate::windows::CallingConvention32::Cdecl,
            1,
        )
        .unwrap();
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.compare_exchange16(u32::MAX, 0, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.compare_exchange_resident16(engine.key, home_id, u32::MAX, 0, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    let outer = engine.pending_call.take().unwrap();
    let token = outer.token + 1;
    engine.callback = Some(SuspendedCallback {
        parent: None,
        outer,
        record: SuspendedRecord::Resident {
            record: crate::abi::resident_callback::ResidentCallbackRecord32 {
                token,
                outer_token: token - 1,
                phase: 1,
                outcome: 0,
                entry_pc: 0x3000,
                entry_esp: 0x8000,
                return_pc: 0x1000,
                return_id: 8,
                stack_words: 0,
                result: 0,
                outer_unit_id: outer_id,
                callback_unit_id: home_id,
            },
            authorized: false,
            active_unit_id: home_id,
        },
    });
    assert_eq!(
        engine.compare_exchange16(0x5000, 0, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.compare_exchange_resident16(engine.key, home_id, 0x5000, 0, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    if let SuspendedRecord::Resident { authorized, .. } =
        &mut engine.callback.as_mut().unwrap().record
    {
        *authorized = true;
    }
    assert_eq!(
        engine.compare_exchange_resident16(engine.key, outer_id, 0x5000, 0, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.compare_exchange16(0x5000, 0, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    for (expected, desired, committed) in [
        (0xffff_3321, 0xdead_beef, 0x3322_u16),
        (0xffff_3322, 0xffff_8180, 0x8180),
    ] {
        let before = engine.arena().to_vec();
        assert_eq!(
            engine.compare_exchange_resident16(engine.key, home_id, 0x5001, expected, desired),
            Ok(StoreCompletion::Complete)
        );
        byte_store_helper_only(&engine, &before, 6, [0, 0x3322, 0, 0, 0, 2]);
        assert_eq!(
            byte_store_ram(&engine, 0x5000, 4),
            [
                0x11,
                committed.to_le_bytes()[0],
                committed.to_le_bytes()[1],
                0x44
            ]
        );
    }
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1000), &[0x0f])
        .unwrap();
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.compare_exchange_resident16(engine.key, home_id, 0x5001, 0x8180, 0),
        Err(HostError::Resident(RegistryError::CodeInvalidated))
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(byte_store_ram(&engine, 0x5000, 4), [0x11, 0x80, 0x81, 0x44]);
    engine.exit_code = Some(7);
    assert_eq!(
        engine.compare_exchange16(u32::MAX, 0, 1),
        Err(HostError::ProcessExited)
    );
    assert_eq!(
        engine.compare_exchange_resident16(0, 0, u32::MAX, 0, 1),
        Err(HostError::ProcessExited)
    );
    assert_eq!(engine.arena(), before);
    engine.close();
    assert_eq!(
        engine.compare_exchange16(u32::MAX, 0, 1),
        Err(HostError::Closed)
    );
    assert_eq!(
        engine.compare_exchange_resident16(0, 0, u32::MAX, 0, 1),
        Err(HostError::Closed)
    );
    assert_eq!(engine.arena(), before);
}
#[test]
fn word_exchange_add_exhaustion_preserves_every_published_owner_and_snapshot() {
    for resident in [false, true] {
        let (mut engine, outer, home) = byte_store_fixture();
        engine.unmap(0x8000, 1).unwrap();
        engine.map(0xffff_f000, 1, 7).unwrap();
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(u32::MAX - 1), &[0x5a, 0xa5])
            .unwrap();
        let bases = [0x1000, 0x3000, 0x5000, 0xffff_f000];
        let ram = bases.map(|base| byte_store_ram(&engine, base, 4096));
        let snapshots = bases.map(|base| {
            engine
                .memory()
                .unwrap()
                .snapshot_code(GuestAddress(base), 4096)
                .unwrap()
        });
        let modules = [
            engine.artifact_bytes().unwrap(),
            engine.resident_bytes(outer).unwrap(),
            engine.resident_bytes(home).unwrap(),
        ]
        .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize));
        let generation = engine.generation();
        let identity = engine.memory().unwrap().identity();
        let epoch = engine.memory().unwrap().code_validation_version();
        engine.memory.as_mut().unwrap().exhaust_versions_for_test();
        for address in [0x1000, 0x5001, u32::MAX - 1] {
            for source in [0, u32::MAX] {
                let before = engine.arena().to_vec();
                assert_eq!(
                    if resident {
                        engine.exchange_add_resident16(engine.key, home, address, source)
                    } else {
                        engine.exchange_add16(address, source)
                    },
                    Ok(StoreCompletion::Complete)
                );
                byte_store_helper_only(&engine, &before, 6, [2, 0, 1, 0, 0, 2]);
                assert_eq!(engine.generation(), generation);
                assert_eq!(engine.memory().unwrap().identity(), identity);
                assert_eq!(engine.memory().unwrap().code_validation_version(), epoch);
                for (index, bytes) in [
                    engine.artifact_bytes().unwrap(),
                    engine.resident_bytes(outer).unwrap(),
                    engine.resident_bytes(home).unwrap(),
                ]
                .into_iter()
                .enumerate()
                {
                    assert_eq!(bytes, modules[index].0);
                    assert_eq!(bytes.as_ptr() as usize, modules[index].1);
                }
                for (index, base) in bases.into_iter().enumerate() {
                    assert_eq!(byte_store_ram(&engine, base, 4096), ram[index]);
                    assert!(engine.memory().unwrap().is_code_current(&snapshots[index]));
                }
                engine.guard(engine.key, generation).unwrap();
                engine.guard_resident(engine.key, outer).unwrap();
                engine.guard_resident(engine.key, home).unwrap();
            }
        }
    }
}

#[test]
fn word_exchange_add_callback_authority_and_lifecycle_precede_all_effects() {
    use crate::process::calls::callback::{SuspendedCallback, SuspendedRecord};
    let (mut engine, outer_id, home_id) = byte_store_fixture();
    byte_store_inject_gate(&mut engine);
    engine
        .capture_resident_call(
            engine.key,
            outer_id,
            crate::windows::CallingConvention32::Cdecl,
            1,
        )
        .unwrap();
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.exchange_add16(u32::MAX, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.exchange_add_resident16(engine.key, home_id, u32::MAX, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    let outer = engine.pending_call.take().unwrap();
    let token = outer.token + 1;
    engine.callback = Some(SuspendedCallback {
        parent: None,
        outer,
        record: SuspendedRecord::Resident {
            record: crate::abi::resident_callback::ResidentCallbackRecord32 {
                token,
                outer_token: token - 1,
                phase: 1,
                outcome: 0,
                entry_pc: 0x3000,
                entry_esp: 0x8000,
                return_pc: 0x1000,
                return_id: 8,
                stack_words: 0,
                result: 0,
                outer_unit_id: outer_id,
                callback_unit_id: home_id,
            },
            authorized: false,
            active_unit_id: home_id,
        },
    });
    assert_eq!(
        engine.exchange_add16(0x5000, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.exchange_add_resident16(engine.key, home_id, 0x5000, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    if let SuspendedRecord::Resident { authorized, .. } =
        &mut engine.callback.as_mut().unwrap().record
    {
        *authorized = true;
    }
    assert_eq!(
        engine.exchange_add_resident16(engine.key, outer_id, 0x5000, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.exchange_add16(0x5000, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    for (source, old, committed) in [
        (0xffff_0000, 0x3322_u16, 0x3322_u16),
        (0xffff_4e5e, 0x3322, 0x8180),
    ] {
        let before = engine.arena().to_vec();
        assert_eq!(
            engine.exchange_add_resident16(engine.key, home_id, 0x5001, source),
            Ok(StoreCompletion::Complete)
        );
        byte_store_helper_only(&engine, &before, 6, [0, u32::from(old), 0, 0, 0, 2]);
        assert_eq!(
            byte_store_ram(&engine, 0x5000, 4),
            [
                0x11,
                committed.to_le_bytes()[0],
                committed.to_le_bytes()[1],
                0x44
            ]
        );
    }
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1000), &[0x0f])
        .unwrap();
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.exchange_add_resident16(engine.key, home_id, 0x5001, 0),
        Err(HostError::Resident(RegistryError::CodeInvalidated))
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(byte_store_ram(&engine, 0x5000, 4), [0x11, 0x80, 0x81, 0x44]);
    engine.exit_code = Some(7);
    assert_eq!(
        engine.exchange_add16(u32::MAX, 1),
        Err(HostError::ProcessExited)
    );
    assert_eq!(
        engine.exchange_add_resident16(0, 0, u32::MAX, 1),
        Err(HostError::ProcessExited)
    );
    assert_eq!(engine.arena(), before);
    engine.close();
    assert_eq!(engine.exchange_add16(u32::MAX, 1), Err(HostError::Closed));
    assert_eq!(
        engine.exchange_add_resident16(0, 0, u32::MAX, 1),
        Err(HostError::Closed)
    );
    assert_eq!(engine.arena(), before);
}

#[test]
fn bit_test_set_exhaustion_preserves_every_published_owner_and_snapshot() {
    for resident in [false, true] {
        let (mut engine, outer, home) = byte_store_fixture();
        engine.unmap(0x8000, 1).unwrap();
        engine.map(0xffff_f000, 1, 7).unwrap();
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(0xffff_fffc), &0x8765_4321_u32.to_le_bytes())
            .unwrap();
        let bases = [0x1000, 0x3000, 0x5000, 0xffff_f000];
        let ram = bases.map(|base| byte_store_ram(&engine, base, 4096));
        let snapshots = bases.map(|base| {
            engine
                .memory()
                .unwrap()
                .snapshot_code(GuestAddress(base), 4096)
                .unwrap()
        });
        let modules = [
            engine.artifact_bytes().unwrap(),
            engine.resident_bytes(outer).unwrap(),
            engine.resident_bytes(home).unwrap(),
        ]
        .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize));
        let generation = engine.generation();
        let identity = engine.memory().unwrap().identity();
        let epoch = engine.memory().unwrap().code_validation_version();
        engine.memory.as_mut().unwrap().exhaust_versions_for_test();
        for address in [0x1000, 0x5000, 0xffff_fffc] {
            for bit in [0, 31] {
                let before = engine.arena().to_vec();
                assert_eq!(
                    if resident {
                        engine.bit_test_set_resident32(engine.key, home, address, bit)
                    } else {
                        engine.bit_test_set32(address, bit)
                    },
                    Ok(StoreCompletion::Complete)
                );
                byte_store_helper_only(&engine, &before, 1, [2, 0, 1, 0, 0, 0]);
                assert_eq!(engine.generation(), generation);
                assert_eq!(engine.memory().unwrap().identity(), identity);
                assert_eq!(engine.memory().unwrap().code_validation_version(), epoch);
                for (index, bytes) in [
                    engine.artifact_bytes().unwrap(),
                    engine.resident_bytes(outer).unwrap(),
                    engine.resident_bytes(home).unwrap(),
                ]
                .into_iter()
                .enumerate()
                {
                    assert_eq!(bytes, modules[index].0);
                    assert_eq!(bytes.as_ptr() as usize, modules[index].1);
                }
                for (index, base) in bases.into_iter().enumerate() {
                    assert_eq!(byte_store_ram(&engine, base, 4096), ram[index]);
                    assert!(engine.memory().unwrap().is_code_current(&snapshots[index]));
                }
                engine.guard(engine.key, generation).unwrap();
                engine.guard_resident(engine.key, outer).unwrap();
                engine.guard_resident(engine.key, home).unwrap();
            }
        }
    }
}

#[test]
fn bit_test_set_callback_authority_and_lifecycle_precede_all_effects() {
    use crate::process::calls::callback::{SuspendedCallback, SuspendedRecord};
    let (mut engine, outer_id, home_id) = byte_store_fixture();
    byte_store_inject_gate(&mut engine);
    engine
        .capture_resident_call(
            engine.key,
            outer_id,
            crate::windows::CallingConvention32::Cdecl,
            1,
        )
        .unwrap();
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.bit_test_set32(u32::MAX, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.bit_test_set_resident32(engine.key, home_id, u32::MAX, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    let outer = engine.pending_call.take().unwrap();
    let token = outer.token + 1;
    engine.callback = Some(SuspendedCallback {
        parent: None,
        outer,
        record: SuspendedRecord::Resident {
            record: crate::abi::resident_callback::ResidentCallbackRecord32 {
                token,
                outer_token: token - 1,
                phase: 1,
                outcome: 0,
                entry_pc: 0x3000,
                entry_esp: 0x8000,
                return_pc: 0x1000,
                return_id: 8,
                stack_words: 0,
                result: 0,
                outer_unit_id: outer_id,
                callback_unit_id: home_id,
            },
            authorized: false,
            active_unit_id: home_id,
        },
    });
    assert_eq!(
        engine.bit_test_set32(0x5000, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.bit_test_set_resident32(engine.key, home_id, 0x5000, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    if let SuspendedRecord::Resident { authorized, .. } =
        &mut engine.callback.as_mut().unwrap().record
    {
        *authorized = true;
    }
    assert_eq!(
        engine.bit_test_set_resident32(engine.key, outer_id, 0x5000, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.bit_test_set32(0x5000, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    for (bit, old, committed) in [
        (0, 0x4433_2211_u32, 0x4433_2211_u32),
        (5, 0x4433_2211, 0x4433_2231),
        (37, 0x4433_2231, 0x4433_2231),
    ] {
        let before = engine.arena().to_vec();
        assert_eq!(
            engine.bit_test_set_resident32(engine.key, home_id, 0x5000, bit),
            Ok(StoreCompletion::Complete)
        );
        byte_store_helper_only(&engine, &before, 1, [0, old, 0, 0, 0, 0]);
        assert_eq!(byte_store_ram(&engine, 0x5000, 4), committed.to_le_bytes());
    }
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1000), &[0x0f])
        .unwrap();
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.bit_test_set_resident32(engine.key, home_id, 0x5000, 0),
        Err(HostError::Resident(RegistryError::CodeInvalidated))
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(byte_store_ram(&engine, 0x5000, 4), [0x31, 0x22, 0x33, 0x44]);
    engine.exit_code = Some(7);
    assert_eq!(
        engine.bit_test_set32(u32::MAX, 1),
        Err(HostError::ProcessExited)
    );
    assert_eq!(
        engine.bit_test_set_resident32(0, 0, u32::MAX, 1),
        Err(HostError::ProcessExited)
    );
    assert_eq!(engine.arena(), before);
    engine.close();
    assert_eq!(engine.bit_test_set32(u32::MAX, 1), Err(HostError::Closed));
    assert_eq!(
        engine.bit_test_set_resident32(0, 0, u32::MAX, 1),
        Err(HostError::Closed)
    );
    assert_eq!(engine.arena(), before);
}

#[test]
fn bit_test_reset_exhaustion_preserves_every_published_owner_and_snapshot() {
    for resident in [false, true] {
        let (mut engine, outer, home) = byte_store_fixture();
        engine.unmap(0x8000, 1).unwrap();
        engine.map(0xffff_f000, 1, 7).unwrap();
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(0xffff_fffc), &0x8765_4321_u32.to_le_bytes())
            .unwrap();
        let bases = [0x1000, 0x3000, 0x5000, 0xffff_f000];
        let ram = bases.map(|base| byte_store_ram(&engine, base, 4096));
        let snapshots = bases.map(|base| {
            engine
                .memory()
                .unwrap()
                .snapshot_code(GuestAddress(base), 4096)
                .unwrap()
        });
        let modules = [
            engine.artifact_bytes().unwrap(),
            engine.resident_bytes(outer).unwrap(),
            engine.resident_bytes(home).unwrap(),
        ]
        .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize));
        let generation = engine.generation();
        let identity = engine.memory().unwrap().identity();
        let epoch = engine.memory().unwrap().code_validation_version();
        engine.memory.as_mut().unwrap().exhaust_versions_for_test();
        for address in [0x1000, 0x5000, 0xffff_fffc] {
            for bit in [0, 31] {
                let before = engine.arena().to_vec();
                assert_eq!(
                    if resident {
                        engine.bit_test_reset_resident32(engine.key, home, address, bit)
                    } else {
                        engine.bit_test_reset32(address, bit)
                    },
                    Ok(StoreCompletion::Complete)
                );
                byte_store_helper_only(&engine, &before, 1, [2, 0, 1, 0, 0, 0]);
                assert_eq!(engine.generation(), generation);
                assert_eq!(engine.memory().unwrap().identity(), identity);
                assert_eq!(engine.memory().unwrap().code_validation_version(), epoch);
                for (index, bytes) in [
                    engine.artifact_bytes().unwrap(),
                    engine.resident_bytes(outer).unwrap(),
                    engine.resident_bytes(home).unwrap(),
                ]
                .into_iter()
                .enumerate()
                {
                    assert_eq!(bytes, modules[index].0);
                    assert_eq!(bytes.as_ptr() as usize, modules[index].1);
                }
                for (index, base) in bases.into_iter().enumerate() {
                    assert_eq!(byte_store_ram(&engine, base, 4096), ram[index]);
                    assert!(engine.memory().unwrap().is_code_current(&snapshots[index]));
                }
                engine.guard(engine.key, generation).unwrap();
                engine.guard_resident(engine.key, outer).unwrap();
                engine.guard_resident(engine.key, home).unwrap();
            }
        }
    }
}

#[test]
fn bit_test_reset_callback_authority_and_lifecycle_precede_all_effects() {
    use crate::process::calls::callback::{SuspendedCallback, SuspendedRecord};
    let (mut engine, outer_id, home_id) = byte_store_fixture();
    byte_store_inject_gate(&mut engine);
    engine
        .capture_resident_call(
            engine.key,
            outer_id,
            crate::windows::CallingConvention32::Cdecl,
            1,
        )
        .unwrap();
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.bit_test_reset32(u32::MAX, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.bit_test_reset_resident32(engine.key, home_id, u32::MAX, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    let outer = engine.pending_call.take().unwrap();
    let token = outer.token + 1;
    engine.callback = Some(SuspendedCallback {
        parent: None,
        outer,
        record: SuspendedRecord::Resident {
            record: crate::abi::resident_callback::ResidentCallbackRecord32 {
                token,
                outer_token: token - 1,
                phase: 1,
                outcome: 0,
                entry_pc: 0x3000,
                entry_esp: 0x8000,
                return_pc: 0x1000,
                return_id: 8,
                stack_words: 0,
                result: 0,
                outer_unit_id: outer_id,
                callback_unit_id: home_id,
            },
            authorized: false,
            active_unit_id: home_id,
        },
    });
    assert_eq!(
        engine.bit_test_reset32(0x5000, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.bit_test_reset_resident32(engine.key, home_id, 0x5000, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    if let SuspendedRecord::Resident { authorized, .. } =
        &mut engine.callback.as_mut().unwrap().record
    {
        *authorized = true;
    }
    assert_eq!(
        engine.bit_test_reset_resident32(engine.key, outer_id, 0x5000, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.bit_test_reset32(0x5000, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    for (bit, old, committed) in [
        (5, 0x4433_2211_u32, 0x4433_2211_u32),
        (0, 0x4433_2211, 0x4433_2210),
        (32, 0x4433_2210, 0x4433_2210),
    ] {
        let before = engine.arena().to_vec();
        assert_eq!(
            engine.bit_test_reset_resident32(engine.key, home_id, 0x5000, bit),
            Ok(StoreCompletion::Complete)
        );
        byte_store_helper_only(&engine, &before, 1, [0, old, 0, 0, 0, 0]);
        assert_eq!(byte_store_ram(&engine, 0x5000, 4), committed.to_le_bytes());
    }
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1000), &[0x0f])
        .unwrap();
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.bit_test_reset_resident32(engine.key, home_id, 0x5000, 0),
        Err(HostError::Resident(RegistryError::CodeInvalidated))
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(byte_store_ram(&engine, 0x5000, 4), [0x10, 0x22, 0x33, 0x44]);
    engine.exit_code = Some(7);
    assert_eq!(
        engine.bit_test_reset32(u32::MAX, 1),
        Err(HostError::ProcessExited)
    );
    assert_eq!(
        engine.bit_test_reset_resident32(0, 0, u32::MAX, 1),
        Err(HostError::ProcessExited)
    );
    assert_eq!(engine.arena(), before);
    engine.close();
    assert_eq!(engine.bit_test_reset32(u32::MAX, 1), Err(HostError::Closed));
    assert_eq!(
        engine.bit_test_reset_resident32(0, 0, u32::MAX, 1),
        Err(HostError::Closed)
    );
    assert_eq!(engine.arena(), before);
}

#[test]
fn atomic_or_exhaustion_preserves_every_published_owner_and_snapshot() {
    for resident in [false, true] {
        let (mut engine, outer, home) = byte_store_fixture();
        engine.unmap(0x8000, 1).unwrap();
        engine.map(0xffff_f000, 1, 7).unwrap();
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(0xffff_fffc), &0x8765_4321_u32.to_le_bytes())
            .unwrap();
        let bases = [0x1000, 0x3000, 0x5000, 0xffff_f000];
        let ram = bases.map(|base| byte_store_ram(&engine, base, 4096));
        let snapshots = bases.map(|base| {
            engine
                .memory()
                .unwrap()
                .snapshot_code(GuestAddress(base), 4096)
                .unwrap()
        });
        let modules = [
            engine.artifact_bytes().unwrap(),
            engine.resident_bytes(outer).unwrap(),
            engine.resident_bytes(home).unwrap(),
        ]
        .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize));
        let generation = engine.generation();
        let identity = engine.memory().unwrap().identity();
        let epoch = engine.memory().unwrap().code_validation_version();
        engine.memory.as_mut().unwrap().exhaust_versions_for_test();
        for address in [0x1000, 0x5000, 0xffff_fffc] {
            for mask in [0, 0x20, 0xffff_ffff] {
                let before = engine.arena().to_vec();
                assert_eq!(
                    if resident {
                        engine.atomic_or_resident_u32(engine.key, home, address, mask)
                    } else {
                        engine.atomic_or_u32(address, mask)
                    },
                    Ok(StoreCompletion::Complete)
                );
                byte_store_helper_only(&engine, &before, 1, [2, 0, 1, 0, 0, 0]);
                assert_eq!(engine.generation(), generation);
                assert_eq!(engine.memory().unwrap().identity(), identity);
                assert_eq!(engine.memory().unwrap().code_validation_version(), epoch);
                for (index, bytes) in [
                    engine.artifact_bytes().unwrap(),
                    engine.resident_bytes(outer).unwrap(),
                    engine.resident_bytes(home).unwrap(),
                ]
                .into_iter()
                .enumerate()
                {
                    assert_eq!(bytes, modules[index].0);
                    assert_eq!(bytes.as_ptr() as usize, modules[index].1);
                }
                for (index, base) in bases.into_iter().enumerate() {
                    assert_eq!(byte_store_ram(&engine, base, 4096), ram[index]);
                    assert!(engine.memory().unwrap().is_code_current(&snapshots[index]));
                }
                engine.guard(engine.key, generation).unwrap();
                engine.guard_resident(engine.key, outer).unwrap();
                engine.guard_resident(engine.key, home).unwrap();
            }
        }
    }
}

#[test]
fn atomic_or_callback_authority_and_lifecycle_precede_all_effects() {
    use crate::process::calls::callback::{SuspendedCallback, SuspendedRecord};
    let (mut engine, outer_id, home_id) = byte_store_fixture();
    byte_store_inject_gate(&mut engine);
    engine
        .capture_resident_call(
            engine.key,
            outer_id,
            crate::windows::CallingConvention32::Cdecl,
            1,
        )
        .unwrap();
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.atomic_or_u32(u32::MAX, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.atomic_or_resident_u32(engine.key, home_id, u32::MAX, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    let outer = engine.pending_call.take().unwrap();
    let token = outer.token + 1;
    engine.callback = Some(SuspendedCallback {
        parent: None,
        outer,
        record: SuspendedRecord::Resident {
            record: crate::abi::resident_callback::ResidentCallbackRecord32 {
                token,
                outer_token: token - 1,
                phase: 1,
                outcome: 0,
                entry_pc: 0x3000,
                entry_esp: 0x8000,
                return_pc: 0x1000,
                return_id: 8,
                stack_words: 0,
                result: 0,
                outer_unit_id: outer_id,
                callback_unit_id: home_id,
            },
            authorized: false,
            active_unit_id: home_id,
        },
    });
    assert_eq!(
        engine.atomic_or_u32(0x5000, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.atomic_or_resident_u32(engine.key, home_id, 0x5000, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    if let SuspendedRecord::Resident { authorized, .. } =
        &mut engine.callback.as_mut().unwrap().record
    {
        *authorized = true;
    }
    assert_eq!(
        engine.atomic_or_resident_u32(engine.key, outer_id, 0x5000, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.atomic_or_u32(0x5000, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    for (mask, old, committed) in [
        (0, 0x4433_2211_u32, 0x4433_2211_u32),
        (0x20, 0x4433_2211, 0x4433_2231),
        (0xffff_ff80, 0x4433_2231, 0xffff_ffb1),
    ] {
        let before = engine.arena().to_vec();
        assert_eq!(
            engine.atomic_or_resident_u32(engine.key, home_id, 0x5000, mask),
            Ok(StoreCompletion::Complete)
        );
        byte_store_helper_only(&engine, &before, 1, [0, old, 0, 0, 0, 0]);
        assert_eq!(byte_store_ram(&engine, 0x5000, 4), committed.to_le_bytes());
    }
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1000), &[0x0f])
        .unwrap();
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.atomic_or_resident_u32(engine.key, home_id, 0x5000, 0),
        Err(HostError::Resident(RegistryError::CodeInvalidated))
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(byte_store_ram(&engine, 0x5000, 4), [0xb1, 0xff, 0xff, 0xff]);
    engine.exit_code = Some(7);
    assert_eq!(
        engine.atomic_or_u32(u32::MAX, 1),
        Err(HostError::ProcessExited)
    );
    assert_eq!(
        engine.atomic_or_resident_u32(0, 0, u32::MAX, 1),
        Err(HostError::ProcessExited)
    );
    assert_eq!(engine.arena(), before);
    engine.close();
    assert_eq!(engine.atomic_or_u32(u32::MAX, 1), Err(HostError::Closed));
    assert_eq!(
        engine.atomic_or_resident_u32(0, 0, u32::MAX, 1),
        Err(HostError::Closed)
    );
    assert_eq!(engine.arena(), before);
}

#[test]
fn atomic_and_exhaustion_preserves_every_published_owner_and_snapshot() {
    for resident in [false, true] {
        let (mut engine, outer, home) = byte_store_fixture();
        engine.unmap(0x8000, 1).unwrap();
        engine.map(0xffff_f000, 1, 7).unwrap();
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(0xffff_fffc), &0x8765_4321_u32.to_le_bytes())
            .unwrap();
        let bases = [0x1000, 0x3000, 0x5000, 0xffff_f000];
        let ram = bases.map(|base| byte_store_ram(&engine, base, 4096));
        let snapshots = bases.map(|base| {
            engine
                .memory()
                .unwrap()
                .snapshot_code(GuestAddress(base), 4096)
                .unwrap()
        });
        let modules = [
            engine.artifact_bytes().unwrap(),
            engine.resident_bytes(outer).unwrap(),
            engine.resident_bytes(home).unwrap(),
        ]
        .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize));
        let generation = engine.generation();
        let identity = engine.memory().unwrap().identity();
        let epoch = engine.memory().unwrap().code_validation_version();
        engine.memory.as_mut().unwrap().exhaust_versions_for_test();
        for address in [0x1000, 0x5000, 0xffff_fffc] {
            for mask in [0, 0xffff_ffdf, 0xffff_ffff] {
                let before = engine.arena().to_vec();
                assert_eq!(
                    if resident {
                        engine.atomic_and_resident_u32(engine.key, home, address, mask)
                    } else {
                        engine.atomic_and_u32(address, mask)
                    },
                    Ok(StoreCompletion::Complete)
                );
                byte_store_helper_only(&engine, &before, 1, [2, 0, 1, 0, 0, 0]);
                assert_eq!(engine.generation(), generation);
                assert_eq!(engine.memory().unwrap().identity(), identity);
                assert_eq!(engine.memory().unwrap().code_validation_version(), epoch);
                for (index, bytes) in [
                    engine.artifact_bytes().unwrap(),
                    engine.resident_bytes(outer).unwrap(),
                    engine.resident_bytes(home).unwrap(),
                ]
                .into_iter()
                .enumerate()
                {
                    assert_eq!(bytes, modules[index].0);
                    assert_eq!(bytes.as_ptr() as usize, modules[index].1);
                }
                for (index, base) in bases.into_iter().enumerate() {
                    assert_eq!(byte_store_ram(&engine, base, 4096), ram[index]);
                    assert!(engine.memory().unwrap().is_code_current(&snapshots[index]));
                }
                engine.guard(engine.key, generation).unwrap();
                engine.guard_resident(engine.key, outer).unwrap();
                engine.guard_resident(engine.key, home).unwrap();
            }
        }
    }
}

#[test]
fn atomic_and_callback_authority_and_lifecycle_precede_all_effects() {
    use crate::process::calls::callback::{SuspendedCallback, SuspendedRecord};
    let (mut engine, outer_id, home_id) = byte_store_fixture();
    byte_store_inject_gate(&mut engine);
    engine
        .capture_resident_call(
            engine.key,
            outer_id,
            crate::windows::CallingConvention32::Cdecl,
            1,
        )
        .unwrap();
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.atomic_and_u32(u32::MAX, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.atomic_and_resident_u32(engine.key, home_id, u32::MAX, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    let outer = engine.pending_call.take().unwrap();
    let token = outer.token + 1;
    engine.callback = Some(SuspendedCallback {
        parent: None,
        outer,
        record: SuspendedRecord::Resident {
            record: crate::abi::resident_callback::ResidentCallbackRecord32 {
                token,
                outer_token: token - 1,
                phase: 1,
                outcome: 0,
                entry_pc: 0x3000,
                entry_esp: 0x8000,
                return_pc: 0x1000,
                return_id: 8,
                stack_words: 0,
                result: 0,
                outer_unit_id: outer_id,
                callback_unit_id: home_id,
            },
            authorized: false,
            active_unit_id: home_id,
        },
    });
    assert_eq!(
        engine.atomic_and_u32(0x5000, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.atomic_and_resident_u32(engine.key, home_id, 0x5000, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    if let SuspendedRecord::Resident { authorized, .. } =
        &mut engine.callback.as_mut().unwrap().record
    {
        *authorized = true;
    }
    assert_eq!(
        engine.atomic_and_resident_u32(engine.key, outer_id, 0x5000, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.atomic_and_u32(0x5000, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    for (mask, old, committed) in [
        (0xffff_ffff, 0x4433_2211_u32, 0x4433_2211_u32),
        (0xffff_ffdf, 0x4433_2211, 0x4433_2211),
        (0xffff_ff80, 0x4433_2211, 0x4433_2200),
        (0, 0x4433_2200, 0),
        (0, 0, 0),
    ] {
        let before = engine.arena().to_vec();
        assert_eq!(
            engine.atomic_and_resident_u32(engine.key, home_id, 0x5000, mask),
            Ok(StoreCompletion::Complete)
        );
        byte_store_helper_only(&engine, &before, 1, [0, old, 0, 0, 0, 0]);
        assert_eq!(byte_store_ram(&engine, 0x5000, 4), committed.to_le_bytes());
    }
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1000), &[0x0f])
        .unwrap();
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.atomic_and_resident_u32(engine.key, home_id, 0x5000, 0),
        Err(HostError::Resident(RegistryError::CodeInvalidated))
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(byte_store_ram(&engine, 0x5000, 4), [0, 0, 0, 0]);
    engine.exit_code = Some(7);
    assert_eq!(
        engine.atomic_and_u32(u32::MAX, 1),
        Err(HostError::ProcessExited)
    );
    assert_eq!(
        engine.atomic_and_resident_u32(0, 0, u32::MAX, 1),
        Err(HostError::ProcessExited)
    );
    assert_eq!(engine.arena(), before);
    engine.close();
    assert_eq!(engine.atomic_and_u32(u32::MAX, 1), Err(HostError::Closed));
    assert_eq!(
        engine.atomic_and_resident_u32(0, 0, u32::MAX, 1),
        Err(HostError::Closed)
    );
    assert_eq!(engine.arena(), before);
}

#[test]
fn atomic_xor_exhaustion_preserves_every_published_owner_and_snapshot() {
    for resident in [false, true] {
        let (mut engine, outer, home) = byte_store_fixture();
        engine.unmap(0x8000, 1).unwrap();
        engine.map(0xffff_f000, 1, 7).unwrap();
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(0xffff_fffc), &0x8765_4321_u32.to_le_bytes())
            .unwrap();
        let bases = [0x1000, 0x3000, 0x5000, 0xffff_f000];
        let ram = bases.map(|base| byte_store_ram(&engine, base, 4096));
        let snapshots = bases.map(|base| {
            engine
                .memory()
                .unwrap()
                .snapshot_code(GuestAddress(base), 4096)
                .unwrap()
        });
        let modules = [
            engine.artifact_bytes().unwrap(),
            engine.resident_bytes(outer).unwrap(),
            engine.resident_bytes(home).unwrap(),
        ]
        .map(|bytes| (bytes.to_vec(), bytes.as_ptr() as usize));
        let generation = engine.generation();
        let identity = engine.memory().unwrap().identity();
        let epoch = engine.memory().unwrap().code_validation_version();
        engine.memory.as_mut().unwrap().exhaust_versions_for_test();
        for address in [0x1000, 0x5000, 0xffff_fffc] {
            for mask in [0, 0x20, 0xffff_ffff] {
                let before = engine.arena().to_vec();
                assert_eq!(
                    if resident {
                        engine.atomic_xor_resident_u32(engine.key, home, address, mask)
                    } else {
                        engine.atomic_xor_u32(address, mask)
                    },
                    Ok(StoreCompletion::Complete)
                );
                byte_store_helper_only(&engine, &before, 1, [2, 0, 1, 0, 0, 0]);
                assert_eq!(engine.generation(), generation);
                assert_eq!(engine.memory().unwrap().identity(), identity);
                assert_eq!(engine.memory().unwrap().code_validation_version(), epoch);
                for (index, bytes) in [
                    engine.artifact_bytes().unwrap(),
                    engine.resident_bytes(outer).unwrap(),
                    engine.resident_bytes(home).unwrap(),
                ]
                .into_iter()
                .enumerate()
                {
                    assert_eq!(bytes, modules[index].0);
                    assert_eq!(bytes.as_ptr() as usize, modules[index].1);
                }
                for (index, base) in bases.into_iter().enumerate() {
                    assert_eq!(byte_store_ram(&engine, base, 4096), ram[index]);
                    assert!(engine.memory().unwrap().is_code_current(&snapshots[index]));
                }
                engine.guard(engine.key, generation).unwrap();
                engine.guard_resident(engine.key, outer).unwrap();
                engine.guard_resident(engine.key, home).unwrap();
            }
        }
    }
}

#[test]
fn atomic_xor_callback_authority_and_lifecycle_precede_all_effects() {
    use crate::process::calls::callback::{SuspendedCallback, SuspendedRecord};
    let (mut engine, outer_id, home_id) = byte_store_fixture();
    byte_store_inject_gate(&mut engine);
    engine
        .capture_resident_call(
            engine.key,
            outer_id,
            crate::windows::CallingConvention32::Cdecl,
            1,
        )
        .unwrap();
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.atomic_xor_u32(u32::MAX, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.atomic_xor_resident_u32(engine.key, home_id, u32::MAX, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    let outer = engine.pending_call.take().unwrap();
    let token = outer.token + 1;
    engine.callback = Some(SuspendedCallback {
        parent: None,
        outer,
        record: SuspendedRecord::Resident {
            record: crate::abi::resident_callback::ResidentCallbackRecord32 {
                token,
                outer_token: token - 1,
                phase: 1,
                outcome: 0,
                entry_pc: 0x3000,
                entry_esp: 0x8000,
                return_pc: 0x1000,
                return_id: 8,
                stack_words: 0,
                result: 0,
                outer_unit_id: outer_id,
                callback_unit_id: home_id,
            },
            authorized: false,
            active_unit_id: home_id,
        },
    });
    assert_eq!(
        engine.atomic_xor_u32(0x5000, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.atomic_xor_resident_u32(engine.key, home_id, 0x5000, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    if let SuspendedRecord::Resident { authorized, .. } =
        &mut engine.callback.as_mut().unwrap().record
    {
        *authorized = true;
    }
    assert_eq!(
        engine.atomic_xor_resident_u32(engine.key, outer_id, 0x5000, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.atomic_xor_u32(0x5000, 1),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.arena(), before);
    for (mask, old, committed) in [
        (0, 0x4433_2211_u32, 0x4433_2211_u32),
        (0x20, 0x4433_2211, 0x4433_2231),
        (0x20, 0x4433_2231, 0x4433_2211),
        (0xffff_ffff, 0x4433_2211, 0xbbcc_ddee),
        (0xffff_ffff, 0xbbcc_ddee, 0x4433_2211),
        (0x4433_2211, 0x4433_2211, 0),
        (0, 0, 0),
    ] {
        let before = engine.arena().to_vec();
        assert_eq!(
            engine.atomic_xor_resident_u32(engine.key, home_id, 0x5000, mask),
            Ok(StoreCompletion::Complete)
        );
        byte_store_helper_only(&engine, &before, 1, [0, old, 0, 0, 0, 0]);
        assert_eq!(byte_store_ram(&engine, 0x5000, 4), committed.to_le_bytes());
    }
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1000), &[0x0f])
        .unwrap();
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.atomic_xor_resident_u32(engine.key, home_id, 0x5000, 0),
        Err(HostError::Resident(RegistryError::CodeInvalidated))
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(byte_store_ram(&engine, 0x5000, 4), [0, 0, 0, 0]);
    engine.exit_code = Some(7);
    assert_eq!(
        engine.atomic_xor_u32(u32::MAX, 1),
        Err(HostError::ProcessExited)
    );
    assert_eq!(
        engine.atomic_xor_resident_u32(0, 0, u32::MAX, 1),
        Err(HostError::ProcessExited)
    );
    assert_eq!(engine.arena(), before);
    engine.close();
    assert_eq!(engine.atomic_xor_u32(u32::MAX, 1), Err(HostError::Closed));
    assert_eq!(
        engine.atomic_xor_resident_u32(0, 0, u32::MAX, 1),
        Err(HostError::Closed)
    );
    assert_eq!(engine.arena(), before);
}
