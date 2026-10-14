use super::*;
use crate::{
    abi::{
        arena::HELPER_OFFSET,
        x86::{StateMetadata, encode_state_v3},
    },
    cpu::x86::{EnteredEvent32, State32},
};

fn request_engine(version: u16) -> EngineInstance {
    let mut engine = EngineInstance::new(1, 0x45564e54).unwrap();
    let mut state = State32 {
        eip: 0x1234,
        eflags: 0x10102,
        ..State32::default()
    };
    state.registers[4] = 0x7800;
    let arena = engine.arena_mut().unwrap();
    encode_state_v3(&state, &mut arena[..56]).unwrap();
    let mut bytes = [0; 56];
    bytes.copy_from_slice(&arena[..56]);
    StateMetadata {
        ss_debug_shadow: true,
    }
    .write(&mut bytes);
    arena[..56].copy_from_slice(&bytes);
    arena[56..96].copy_from_slice(&[
        82, 51, 69, 88, 6, 0, 1, 0, 40, 0, 0, 0, 0, 0, 0, 0, 11, 0, 0, 0, 9, 0, 0, 0, 72, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ]);
    arena[60..62].copy_from_slice(&version.to_le_bytes());
    let event = &mut arena[TRANSFER_OFFSET..TRANSFER_OFFSET + EVENT_REQUEST_SIZE];
    event.fill(0);
    event[..16].copy_from_slice(&[82, 51, 69, 86, 1, 0, 1, 0, 64, 0, 0, 0, 0, 0, 0, 0]);
    for (offset, value) in [
        (16, 6_u32),
        (20, 0x48),
        (32, 0x1234),
        (36, 0x7800),
        (40, 0x10102),
        (44, 0x1b),
    ] {
        event[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    engine
}

#[test]
fn entry_publication_preserves_retirement_and_exit_capabilities_and_clears_shadow() {
    for version in [6, 7, 9] {
        let mut engine = request_engine(version);
        let before = engine.arena().to_vec();
        let prepared = engine.prepare_event_request().unwrap();
        let completion = engine.publish_event_entry(
            prepared,
            EventDeliveryResult32::Entered(EnteredEvent32 {
                target: 0x2345,
                esp: 0x77f0,
                flags: 2,
                frame_info: 0x304,
                code_invalidated: false,
            }),
        );
        assert_eq!(completion, StoreCompletion::Complete);
        let mut expected = before;
        for (offset, value) in [
            (12, 0_u32),
            (32, 0x77f0),
            (48, 0x2345),
            (52, 2),
            (72, 3),
            (80, 0),
        ] {
            expected[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        assert_eq!(engine.arena(), expected);
        assert_eq!(
            decode_exit(&engine.arena()[56..96]),
            Ok(ExecutionExit {
                retired: 9,
                reason: ExitReason::NeedCode
            })
        );
        assert_eq!(&engine.arena()[60..62], &version.to_le_bytes());
    }
}

#[test]
fn missing_authority_result_preserves_state_exit_and_unrelated_arena() {
    let mut engine = request_engine(9);
    let before = engine.arena().to_vec();
    let prepared = engine.prepare_event_request().unwrap();
    let result = EventDeliveryResult32::NeedsTaskOrPrivilegeAuthority {
        kind: crate::cpu::x86::EventAuthorityKind32::Privilege,
        selector: 0x48,
    };
    assert_eq!(
        engine.publish_event_entry(prepared, result),
        StoreCompletion::Complete
    );
    assert_eq!(engine.arena(), before);
}

#[test]
fn snapshot_mismatch_and_wrong_owner_reject_before_any_packet_or_ram_publication() {
    for offset in [32, 36, 40, 44] {
        let mut engine = request_engine(7);
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + offset] ^= 1;
        let before = engine.arena().to_vec();
        assert!(matches!(
            engine.prepare_event_request(),
            Err(HostError::InvalidRequest)
        ));
        assert_eq!(engine.arena(), before);
    }
    let mut engine = request_engine(7);
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + EVENT_REQUEST_SIZE].fill(0xff);
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.deliver_exception_request(),
        Err(HostError::InvalidArtifact)
    );
    assert_eq!(
        engine.deliver_exception_resident_request(0, 0),
        Err(HostError::InvalidArtifact)
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(&engine.arena()[HELPER_OFFSET..HELPER_OFFSET + 4], b"R3MH");
}
