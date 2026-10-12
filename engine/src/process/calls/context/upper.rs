use super::ExtendedCpuState;
use crate::process::{CallError, EngineInstance, HostError};

fn extended_arena() -> Vec<u8> {
    let mut arena = EngineInstance::new(1, 91).unwrap().arena().to_vec();
    arena.resize(4756, 0);
    arena[4612..4616].copy_from_slice(b"R3YU");
    arena[4616..4620].copy_from_slice(&[1, 0, 1, 0]);
    arena[4620..4624].copy_from_slice(&144_u32.to_le_bytes());
    for (index, byte) in arena[4628..4756].iter_mut().enumerate() {
        *byte = (index as u8).wrapping_mul(73).wrapping_add(11);
    }
    arena
}

#[test]
fn changed_upper_payload_invalidates_captured_cpu_context() {
    let mut arena = extended_arena();
    let saved = ExtendedCpuState::capture(&arena).unwrap();
    assert!(saved.matches(&arena));
    arena[4628] ^= 0x80;
    assert!(
        !saved.matches(&arena),
        "pending completion must detect changed upper state"
    );
}

#[test]
fn restoring_cpu_context_restores_every_upper_bit_and_preserves_runtime_output() {
    let mut arena = extended_arena();
    let expected = arena[4612..4756].to_vec();
    let saved = ExtendedCpuState::capture(&arena).unwrap();
    arena[4628..4756].fill(0x5a);
    arena[140] = 0x69;
    saved.restore(&mut arena);
    assert_eq!(&arena[4612..4756], expected);
    assert_eq!(arena[140], 0x69);
}

#[test]
fn malformed_upper_record_is_refused_before_context_capture() {
    for offset in [4612, 4616, 4618, 4620, 4624] {
        let mut arena = extended_arena();
        arena[offset] ^= 0x40;
        let before = arena.clone();
        assert_eq!(
            ExtendedCpuState::capture(&arena),
            Err(HostError::Call(CallError::InvalidStop))
        );
        assert_eq!(arena, before);
    }
}

#[test]
fn truncated_upper_context_is_refused_without_panicking_or_matching() {
    let arena = extended_arena();
    let captured = ExtendedCpuState::capture(&arena).unwrap();
    for length in [0, 56, 4612, 4755] {
        assert_eq!(
            ExtendedCpuState::capture(&arena[..length]),
            Err(HostError::Call(CallError::InvalidStop))
        );
        assert!(!captured.matches(&arena[..length]));
    }
}

#[test]
fn upper_state_survives_pending_completion_and_aborted_callback() {
    use crate::{
        abi::{
            arena::TRANSFER_OFFSET,
            x86::{encode_exit_v3, encode_state},
        },
        cpu::{ExecutionExit, ExitReason, x86::State32},
        windows::CallingConvention32,
    };
    let mut engine = EngineInstance::new(2, 91).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.map(0x8000, 1, 3).unwrap();
    for (address, code) in [
        (0x1000, &[0x0f, 0x0b][..]),
        (0x1100, &[0x90][..]),
        (0x1200, &[0x0f, 0x0b][..]),
    ] {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + code.len()]
            .copy_from_slice(code);
        engine.upload(address, code.len() as u32).unwrap();
    }
    for (index, value) in [0x1000_u32, 2, 0x1100, 1, 0x1200, 2, 0x1000, 17, 0x1200, 18]
        .into_iter()
        .enumerate()
    {
        engine.arena_mut().unwrap()[140 + index * 4..144 + index * 4]
            .copy_from_slice(&value.to_le_bytes());
    }
    let generation = engine.compile_with_gates(3, 2).unwrap();
    engine.write32(0x8010, 0x2000).unwrap();
    let mut state = State32 {
        eip: 0x1000,
        ..State32::default()
    };
    state.registers[4] = 0x8010;
    encode_state(&state, &mut engine.arena_mut().unwrap()[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 1,
            reason: ExitReason::Gate { id: 17 },
        },
        &mut engine.arena_mut().unwrap()[56..96],
    )
    .unwrap();
    let upper = extended_arena()[4612..4756].to_vec();
    engine.arena_mut().unwrap()[4612..4756].copy_from_slice(&upper);
    let outer = engine
        .capture_call(91, generation, CallingConvention32::Cdecl, 0)
        .unwrap();
    engine.arena_mut().unwrap()[4628] ^= 0x80;
    let changed = engine.arena().to_vec();
    assert_eq!(
        engine.complete_call(91, generation, outer.token, 42),
        Err(HostError::Call(CallError::StateChanged))
    );
    assert_eq!(engine.arena(), changed);
    engine.arena_mut().unwrap()[4612..4756].copy_from_slice(&upper);
    let callback = engine
        .begin_callback(91, generation, outer.token, 0x1100, 0x1200, 18, &[])
        .unwrap();
    engine.arena_mut().unwrap()[4628..4756].fill(0x5a);
    engine.abort_callback(91, callback.token).unwrap();
    assert_eq!(&engine.arena()[4612..4756], upper);
    engine
        .complete_call(91, generation, outer.token, 42)
        .unwrap();
    assert_eq!(&engine.arena()[4612..4756], upper);
}

#[test]
fn resident_callback_finish_restores_exact_upper_and_rejects_malformed_current_state() {
    use crate::{
        abi::{
            arena::TRANSFER_OFFSET,
            x86::{encode_exit_v3, encode_state},
        },
        cpu::{ExecutionExit, ExitReason, x86::State32},
        windows::CallingConvention32,
    };
    let mut engine = EngineInstance::new(2, 91).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.map(0x8000, 1, 3).unwrap();
    for (pc, code) in [
        (0x1000, &[0x0f, 0x0b][..]),
        (0x1100, &[0x90][..]),
        (0x1200, &[0x0f, 0x0b][..]),
    ] {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + code.len()]
            .copy_from_slice(code);
        engine.upload(pc, code.len() as u32).unwrap();
    }
    let mut ids = Vec::new();
    for (slot, descriptors) in [
        &[(0x1000_u32, 2_u32), (0x1000, 17)][..],
        &[(0x1100, 1), (0x1200, 2), (0x1200, 18)][..],
    ]
    .into_iter()
    .enumerate()
    {
        for (index, (pc, value)) in descriptors.iter().enumerate() {
            engine.arena_mut().unwrap()[140 + index * 8..144 + index * 8]
                .copy_from_slice(&pc.to_le_bytes());
            engine.arena_mut().unwrap()[144 + index * 8..148 + index * 8]
                .copy_from_slice(&value.to_le_bytes());
        }
        let id = engine
            .compile_resident_with_gates(descriptors.len() as u32 - 1, 1)
            .unwrap()
            .get();
        engine
            .acknowledge_resident_installation(91, id, slot as u32)
            .unwrap();
        ids.push(id);
    }
    engine.write32(0x8010, 0x2000).unwrap();
    let mut state = State32 {
        eip: 0x1000,
        ..State32::default()
    };
    state.registers[4] = 0x8010;
    encode_state(&state, &mut engine.arena_mut().unwrap()[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 1,
            reason: ExitReason::Gate { id: 17 },
        },
        &mut engine.arena_mut().unwrap()[56..96],
    )
    .unwrap();
    let upper = extended_arena()[4612..4756].to_vec();
    engine.arena_mut().unwrap()[4612..4756].copy_from_slice(&upper);
    let outer = engine
        .capture_resident_call(91, ids[0], CallingConvention32::Cdecl, 0)
        .unwrap();
    let callback = engine
        .begin_resident_callback(91, ids[0], ids[1], outer.token, 0x1100, 0x1200, 18, &[])
        .unwrap();
    engine
        .authorize_resident_callback(91, ids[1], callback.token)
        .unwrap();
    state.eip = 0x1200;
    state.registers[0] = 42;
    encode_state(&state, &mut engine.arena_mut().unwrap()[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate { id: 18 },
        },
        &mut engine.arena_mut().unwrap()[56..96],
    )
    .unwrap();
    engine.arena_mut().unwrap()[4628..4756].fill(0x6d);
    engine.arena_mut().unwrap()[4624] = 1;
    let malformed = engine.arena().to_vec();
    assert_eq!(
        engine.finish_resident_callback(91, ids[1], callback.token),
        Err(HostError::Call(CallError::InvalidStop))
    );
    assert_eq!(engine.arena(), malformed);
    engine.arena_mut().unwrap()[4624] = 0;
    let result = engine
        .finish_resident_callback(91, ids[1], callback.token)
        .unwrap();
    assert_eq!(result.result, 42);
    assert_eq!(&engine.arena()[4612..4756], upper);
    engine
        .complete_resident_call(91, ids[0], outer.token, 91)
        .unwrap();
    assert_eq!(&engine.arena()[4612..4756], upper);
}
