use ring3_engine::process::EngineInstance;
use ring3_engine::{
    abi::{
        arena::{TRANSFER_OFFSET, X87_OFFSET},
        x86::{X87_SIZE, decode_x87, encode_exit_v3, encode_state, encode_x87},
    },
    cpu::{
        ExecutionExit, ExitReason,
        x86::{State32, X87State},
    },
    process::{CallError, HostError},
    windows::CallingConvention32,
};

#[test]
fn engine_initializes_an_append_only_exact_x87_record() {
    let engine = EngineInstance::new(1, 1).unwrap();
    let arena = engine.arena();
    assert_eq!(arena.len(), 4364);
    let tail = &arena[4236..];
    assert_eq!(
        &tail[..16],
        b"R3FP\x01\x00\x01\x00\x80\x00\x00\x00\x00\x00\x00\x00"
    );
    assert_eq!(&tail[16..24], &[0x7f, 3, 0, 0, 0xff, 0xff, 0, 0]);
    assert!(tail[24..].iter().all(|byte| *byte == 0));
}

fn stopped_gate() -> (EngineInstance, u32) {
    let mut engine = EngineInstance::new(2, 91).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.map(0x8000, 1, 3).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
        .copy_from_slice(&[0x0f, 0x0b]);
    engine.upload(0x1000, 2).unwrap();
    for (index, value) in [0x1000_u32, 2, 0x1000, 77].into_iter().enumerate() {
        let at = TRANSFER_OFFSET + index * 4;
        engine.arena_mut().unwrap()[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    let generation = engine.compile_with_gates(1, 1).unwrap();
    engine.write32(0x8010, 0x1234_5678).unwrap();
    let mut state = State32 {
        eip: 0x1000,
        ..State32::default()
    };
    state.registers[4] = 0x8010;
    encode_state(&state, &mut engine.arena_mut().unwrap()[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 1,
            reason: ExitReason::Gate { id: 77 },
        },
        &mut engine.arena_mut().unwrap()[56..96],
    )
    .unwrap();
    (engine, generation)
}

#[test]
fn exact_payload_roundtrip_and_malformed_records_fail_closed() {
    let state = X87State {
        control: 0x137f,
        status: 0xffff,
        tag: 0x1234,
        opcode: 0x7ff,
        instruction_pointer: 0xffff_ffff,
        data_pointer: 0x89ab_cdef,
        code_selector: 0xfedc,
        data_selector: 0x7654,
        registers: std::array::from_fn(|r| std::array::from_fn(|b| (r * 31 + b * 17) as u8)),
    };
    let mut encoded = [0; X87_SIZE];
    encode_x87(&state, &mut encoded).unwrap();
    assert_eq!(decode_x87(&encoded).unwrap(), state);
    for at in [0, 4, 6, 8, 12, 23, 36, 39, 120, 127] {
        let mut invalid = encoded;
        invalid[at] ^= 0x80;
        assert!(decode_x87(&invalid).is_err(), "offset {at}");
    }
    assert!(decode_x87(&encoded[..127]).is_err());
    let invalid = X87State {
        opcode: 0x800,
        ..state
    };
    let before = encoded;
    assert!(encode_x87(&invalid, &mut encoded).is_err());
    assert_eq!(encoded, before);
}

#[test]
fn captured_calls_fingerprint_every_x87_byte_and_preserve_it_on_completion() {
    let (mut engine, generation) = stopped_gate();
    let state = X87State {
        status: 0xabcd,
        registers: [[0xa5; 10]; 8],
        ..X87State::default()
    };
    encode_x87(&state, &mut engine.arena_mut().unwrap()[X87_OFFSET..]).unwrap();
    let record = engine
        .capture_call(91, generation, CallingConvention32::Cdecl, 0)
        .unwrap();
    let captured = engine.arena()[X87_OFFSET..].to_vec();
    for at in [18, 40, 119, 127] {
        engine.arena_mut().unwrap()[X87_OFFSET + at] ^= 1;
        let before = engine.arena().to_vec();
        assert_eq!(
            engine.complete_call(91, generation, record.token, 17),
            Err(HostError::Call(CallError::StateChanged))
        );
        assert_eq!(engine.arena(), before);
        engine.arena_mut().unwrap()[X87_OFFSET + at] ^= 1;
    }
    engine
        .complete_call(91, generation, record.token, 17)
        .unwrap();
    assert_eq!(&engine.arena()[X87_OFFSET..], captured);
}

#[test]
fn invalid_x87_state_cannot_be_captured_as_a_call() {
    let (mut engine, generation) = stopped_gate();
    engine.arena_mut().unwrap()[X87_OFFSET + 120] = 1;
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.capture_call(91, generation, CallingConvention32::Cdecl, 0),
        Err(HostError::Call(CallError::InvalidStop))
    );
    assert_eq!(engine.arena(), before);
}

#[test]
fn callback_abort_and_return_restore_the_exact_outer_x87_context() {
    let (mut engine, _) = stopped_gate();
    for (address, bytes) in [(0x1100, &[0x90][..]), (0x1200, &[0x0f, 0x0b][..])] {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + bytes.len()]
            .copy_from_slice(bytes);
        engine.upload(address, bytes.len() as u32).unwrap();
    }
    for (index, value) in [0x1000_u32, 2, 0x1100, 1, 0x1200, 2, 0x1000, 77, 0x1200, 78]
        .into_iter()
        .enumerate()
    {
        let at = TRANSFER_OFFSET + index * 4;
        engine.arena_mut().unwrap()[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    let generation = engine.compile_with_gates(3, 2).unwrap();
    let outer = X87State {
        status: 0x4567,
        registers: [[0x8a; 10]; 8],
        ..X87State::default()
    };
    encode_x87(&outer, &mut engine.arena_mut().unwrap()[X87_OFFSET..]).unwrap();
    let call = engine
        .capture_call(91, generation, CallingConvention32::Cdecl, 0)
        .unwrap();
    let callback = engine
        .begin_callback(91, generation, call.token, 0x1100, 0x1200, 78, &[])
        .unwrap();
    assert_eq!(decode_x87(&engine.arena()[X87_OFFSET..]).unwrap(), outer);
    let inner = X87State {
        status: 0xabcd,
        registers: [[0x35; 10]; 8],
        ..X87State::default()
    };
    encode_x87(&inner, &mut engine.arena_mut().unwrap()[X87_OFFSET..]).unwrap();
    engine.abort_callback(91, callback.token).unwrap();
    assert_eq!(decode_x87(&engine.arena()[X87_OFFSET..]).unwrap(), outer);
    let callback = engine
        .begin_callback(91, generation, call.token, 0x1100, 0x1200, 78, &[])
        .unwrap();
    let mut returned = State32 {
        eip: 0x1200,
        ..State32::default()
    };
    returned.registers[4] = 0x8010;
    returned.registers[0] = 0x9876;
    encode_state(&returned, &mut engine.arena_mut().unwrap()[..56]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 1,
            reason: ExitReason::Gate { id: 78 },
        },
        &mut engine.arena_mut().unwrap()[56..96],
    )
    .unwrap();
    encode_x87(&inner, &mut engine.arena_mut().unwrap()[X87_OFFSET..]).unwrap();
    engine.arena_mut().unwrap()[X87_OFFSET + 120] = 1;
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.finish_callback(91, generation, callback.token),
        Err(HostError::Call(CallError::InvalidStop))
    );
    assert_eq!(engine.arena(), before);
    engine.arena_mut().unwrap()[X87_OFFSET + 120] = 0;
    assert_eq!(
        engine
            .finish_callback(91, generation, callback.token)
            .unwrap()
            .result,
        0x9876
    );
    assert_eq!(decode_x87(&engine.arena()[X87_OFFSET..]).unwrap(), outer);
    engine.complete_call(91, generation, call.token, 5).unwrap();
    assert_eq!(decode_x87(&engine.arena()[X87_OFFSET..]).unwrap(), outer);
}
