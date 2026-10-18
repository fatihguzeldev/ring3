use super::*;
use crate::{
    abi::{
        arena::TRANSFER_OFFSET,
        x86::{EXIT_SIZE, STATE_SIZE},
    },
    cpu::x86::{SegmentRegister, State32},
    memory::GuestAddress,
    process::{
        calls::call::{PendingCall, PendingOwner},
        cpu::context::ExtendedCpuState,
    },
    windows::{CallFrame32, CallingConvention32},
};

const KEY: u64 = 0x41555448;

fn fixture() -> (EngineInstance, u64) {
    let mut engine = EngineInstance::new(2, KEY).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.map(0x7000, 1, 3).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1000), &[0xeb, 0xfe])
        .unwrap();
    let transfer = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    transfer[..4].copy_from_slice(&0x1000u32.to_le_bytes());
    transfer[4..].copy_from_slice(&2u32.to_le_bytes());
    let id = engine.compile_resident(1).unwrap().get();
    (engine, id)
}

fn park_call(engine: &mut EngineInstance, id: u64) {
    let mut state = State32::default();
    state.registers[4] = 0x7800;
    let frame = CallFrame32::capture(
        engine.memory().unwrap(),
        state,
        CallingConvention32::Stdcall,
        0,
    )
    .unwrap();
    engine.pending_call = Some(PendingCall {
        token: 1,
        owner: PendingOwner::Resident(id),
        frame,
        state: [0; STATE_SIZE],
        exit: [0; EXIT_SIZE],
        extended: ExtendedCpuState::initial().unwrap(),
    });
}

#[test]
fn resident_material_guard_preserves_key_identity_pending_and_decode_precedence() {
    let (mut engine, id) = fixture();
    assert_eq!(engine.guard_material_resident(KEY, id), Ok(()));
    engine
        .set_segment_base(SegmentRegister::Cs, 0x4000)
        .unwrap();
    assert_eq!(engine.guard_resident(KEY, id), Ok(()));
    assert_eq!(
        engine.guard_material_resident(KEY, id),
        Err(HostError::InvalidArtifact)
    );
    park_call(&mut engine, id);
    let before = engine.arena().to_vec();
    for (key, unit, expected) in [
        (KEY ^ 1, id, HostError::InvalidArtifact),
        (KEY, 0, HostError::Resident(RegistryError::InvalidUnit)),
        (KEY, id, HostError::Call(CallError::Busy)),
    ] {
        assert_eq!(engine.guard_material_resident(key, unit), Err(expected));
        assert_eq!(engine.arena(), before);
    }
}

#[test]
fn resident_material_guard_rechecks_currentness_before_pending_and_never_reuses_stale_region() {
    let (mut engine, id) = fixture();
    assert_eq!(engine.guard_material_resident(KEY, id), Ok(()));
    park_call(&mut engine, id);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1000), &[0x90])
        .unwrap();
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.guard_material_resident(KEY, id),
        Err(HostError::Resident(RegistryError::CodeInvalidated))
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.pending_call.as_ref().unwrap().token, 1);
    engine.close();
    assert_eq!(
        engine.guard_material_resident(KEY ^ 1, id),
        Err(HostError::Closed)
    );
}
