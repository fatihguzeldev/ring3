use ring3_engine::{
    abi::{
        arena::{EXIT_OFFSET, STATE_OFFSET, TRANSFER_OFFSET},
        x86::{EXIT_SIZE, STATE_SIZE, encode_exit_v3, encode_state},
    },
    cpu::{
        ExecutionExit, ExitReason,
        x86::{SegmentRegister, State32},
    },
    process::{CallError, EngineInstance, HostError},
};

fn check_capture(resident: bool, based: bool, stale: bool) {
    let mut engine = EngineInstance::new(2, 91).unwrap();
    let physical = if based { 0x5000 } else { 0x1000 };
    engine.map(physical, 1, 7).unwrap();
    engine.map(0x8000, 1, 3).unwrap();
    engine.write32(0x8010, 0x2000).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
        .copy_from_slice(&[0x0f, 0x0b]);
    engine.upload(physical, 2).unwrap();
    engine
        .set_segment_base(SegmentRegister::Cs, if based { 0x4000 } else { 0 })
        .unwrap();
    for (index, value) in [0x1000_u32, 2, 0x1000, 7].into_iter().enumerate() {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&value.to_le_bytes());
    }
    let owner = if resident {
        engine.compile_resident_with_gates(1, 1).unwrap().get()
    } else {
        u64::from(engine.compile_with_gates(1, 1).unwrap())
    };
    let mut state = State32 {
        eip: 0x1000,
        ..State32::default()
    };
    state.registers[4] = 0x8010;
    encode_state(
        &state,
        &mut engine.arena_mut().unwrap()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE],
    )
    .unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate { id: 7 },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    // the stop describes the real compiled based gate; only live CS changes.
    engine.set_segment_base(SegmentRegister::Cs, 0).unwrap();
    if stale {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET] = 0x0f;
        engine.upload(physical, 1).unwrap();
    }
    let before = engine.arena().to_vec();
    let result = if resident {
        engine.capture_resident_call_raw(91, owner, 1, 0)
    } else {
        engine.capture_call_raw(91, owner as u32, 1, 0)
    };
    if stale {
        let expected = if resident {
            HostError::Resident(ring3_engine::cpu::dbt::RegistryError::CodeInvalidated)
        } else {
            HostError::CodeInvalidated
        };
        assert!(
            matches!(&result, Err(error) if error == &expected),
            "physical stale priority lost: {result:?}"
        );
        assert_eq!(engine.arena(), before);
    } else if based {
        assert!(
            matches!(&result, Err(HostError::Call(CallError::InvalidStop))),
            "wrong-key gate captured: {result:?}"
        );
        assert_eq!(engine.arena(), before);
    } else {
        let record = result.expect("flat current gate remains capturable");
        assert_eq!(record.id, 7);
        assert_ne!(record.token, 0);
    }
}

#[test]
fn replacement_call_capture_requires_gate_decode_key() {
    check_capture(false, true, false);
}

#[test]
fn resident_call_capture_requires_gate_decode_key() {
    check_capture(true, true, false);
}

#[test]
fn replacement_flat_gate_remains_capturable() {
    check_capture(false, false, false);
}

#[test]
fn resident_flat_gate_remains_capturable() {
    check_capture(true, false, false);
}

#[test]
fn replacement_stale_gate_keeps_physical_error_priority() {
    check_capture(false, true, true);
}

#[test]
fn resident_stale_gate_keeps_physical_error_priority() {
    check_capture(true, true, true);
}
