use super::*;
use crate::windows::CallingConvention32;

const KEY: u64 = 91;

fn stop(engine: &mut EngineInstance, pc: u32, gate: u32, esp: u32, eax: u32) {
    let state = crate::cpu::x86::State32 {
        eip: pc,
        registers: [eax, 0x1234, 0x2345, 0x3456, esp, 0x5678, 0x6789, 0x789a],
        eflags: 0x246,
    };
    encode_state(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 7,
            reason: ExitReason::Gate { id: gate },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
}

fn fixture() -> (EngineInstance, u64, u64) {
    let mut engine = EngineInstance::new(2, KEY).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.map(0x8000, 1, 3).unwrap();
    for (pc, bytes) in [
        (0x1000, &[0x0f, 0x0b][..]),
        (0x1100, &[0x90][..]),
        (0x1200, &[0x0f, 0x0b][..]),
        (0x1300, &[0x0f, 0x0b][..]),
        (0x1400, &[0x90][..]),
        (0x1500, &[0x0f, 0x0b][..]),
    ] {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
            .copy_from_slice(bytes);
        engine.upload(pc, bytes.len() as u32).unwrap();
    }
    let mut units = Vec::new();
    for (blocks, gates, fields) in [
        (1, 1, &[0x1000_u32, 2, 0x1000, 17][..]),
        (
            5,
            3,
            &[
                0x1100, 1, 0x1200, 2, 0x1300, 2, 0x1400, 1, 0x1500, 2, 0x1200, 18, 0x1300, 19,
                0x1500, 20,
            ][..],
        ),
    ] {
        for (index, field) in fields.iter().enumerate() {
            let offset = TRANSFER_OFFSET + index * 4;
            engine.arena_mut().unwrap()[offset..offset + 4].copy_from_slice(&field.to_le_bytes());
        }
        units.push(
            engine
                .compile_resident_with_gates(blocks, gates)
                .unwrap()
                .get(),
        );
    }
    (engine, units[0], units[1])
}

fn nested() -> (EngineInstance, u64, u64, u32, u32, u32, Vec<u8>) {
    let (mut engine, outer, home) = fixture();
    engine.write32(0x8100, 0x2000).unwrap();
    stop(&mut engine, 0x1000, 17, 0x8100, 5);
    let outer_call = engine
        .capture_resident_call(KEY, outer, CallingConvention32::Cdecl, 0)
        .unwrap();
    let parent = engine
        .begin_resident_callback(KEY, outer, home, outer_call.token, 0x1100, 0x1200, 18, &[])
        .unwrap();
    engine
        .authorize_resident_callback(KEY, home, parent.token)
        .unwrap();
    engine.write32(0x80f0, 0x1101).unwrap();
    stop(&mut engine, 0x1300, 19, 0x80f0, 6);
    let inner = engine
        .capture_active_resident_callback_call(
            KEY,
            home,
            parent.token,
            CallingConvention32::Cdecl,
            0,
        )
        .unwrap();
    let saved = engine.arena()[..EXIT_OFFSET + EXIT_SIZE].to_vec();
    let child = engine
        .begin_resident_callback(
            KEY,
            home,
            home,
            inner.token,
            0x1400,
            0x1500,
            20,
            &[0xabcdef],
        )
        .unwrap();
    engine
        .authorize_resident_callback(KEY, home, child.token)
        .unwrap();
    assert_eq!(engine.callback.as_ref().unwrap().depth(), 2);
    (
        engine,
        outer,
        home,
        parent.token,
        inner.token,
        child.token,
        saved,
    )
}

#[test]
fn nested_same_unit_callback_restores_authorized_parent_and_captured_inner_call() {
    let (mut engine, outer, home, parent, inner, child, saved) = nested();
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.finish_resident_callback(KEY, home, parent),
        Err(HostError::Call(CallError::InvalidToken))
    );
    assert_eq!(
        engine.abort_callback(KEY, parent),
        Err(HostError::Call(CallError::InvalidToken))
    );
    assert_eq!(engine.arena(), before);
    stop(&mut engine, 0x1500, 20, 0x80f0, 0x12345678);
    let result = engine.finish_resident_callback(KEY, home, child).unwrap();
    assert_eq!(result.result, 0x12345678);
    assert_eq!(&engine.arena()[..EXIT_OFFSET + EXIT_SIZE], saved);
    assert_eq!(engine.callback.as_ref().unwrap().token(), parent);
    assert_eq!(
        engine
            .callback
            .as_ref()
            .unwrap()
            .authorized_resident_active_id(),
        Some(home)
    );
    assert_eq!(engine.pending_call.as_ref().unwrap().token, inner);
    assert_eq!(
        engine.pending_call.as_ref().unwrap().owner,
        PendingOwner::ResidentCallback {
            unit_id: home,
            callback_token: parent
        }
    );
    engine
        .complete_active_resident_callback_call(KEY, home, parent, inner, result.result)
        .unwrap();
    stop(&mut engine, 0x1200, 18, 0x8100, 0x87654321);
    let result = engine.finish_resident_callback(KEY, home, parent).unwrap();
    engine
        .complete_resident_call(KEY, outer, result.outer_token, result.result)
        .unwrap();
    assert!(engine.callback.is_none());
    assert!(engine.pending_call.is_none());
}

#[test]
fn nested_abort_restores_parent_context_and_rejects_the_retired_child_token() {
    let (mut engine, _, _, parent, inner, child, saved) = nested();
    engine.arena_mut().unwrap()[0] ^= 0x5a;
    engine.abort_callback(KEY, child).unwrap();
    assert_eq!(&engine.arena()[..EXIT_OFFSET + EXIT_SIZE], saved);
    assert_eq!(engine.callback.as_ref().unwrap().token(), parent);
    assert_eq!(engine.pending_call.as_ref().unwrap().token, inner);
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.abort_callback(KEY, child),
        Err(HostError::Call(CallError::InvalidToken))
    );
    assert_eq!(engine.arena(), before);
    engine.abandon_call(KEY, inner).unwrap();
    engine.abort_callback(KEY, parent).unwrap();
    assert!(engine.callback.is_none());
    assert_eq!(engine.pending_call.as_ref().unwrap().token, 1);
}
