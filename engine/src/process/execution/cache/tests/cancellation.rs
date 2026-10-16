use super::admission::{KEY, fixture};
use crate::{
    abi::x86::{EXIT_SIZE, STATE_SIZE},
    cpu::x86::State32,
    process::{
        CallError, HostError,
        calls::call::{PendingCall, PendingOwner},
        cpu::context::ExtendedCpuState,
    },
    windows::{CallFrame32, CallingConvention32},
};

#[test]
fn cancellation_preserves_an_uninstalled_candidate_while_a_call_retains_it() {
    let mut engine = fixture();
    let candidate = engine.prepare_cache_entry(KEY, 0x1000).unwrap();
    engine.write32(0x8000, 0x1000).unwrap();
    let frame = CallFrame32::capture(
        engine.memory().unwrap(),
        State32 {
            eip: 0x1000,
            registers: [0, 0, 0, 0, 0x8000, 0, 0, 0],
            eflags: 2,
        },
        CallingConvention32::Stdcall,
        0,
    )
    .unwrap();
    engine.pending_call = Some(PendingCall {
        token: 1,
        owner: PendingOwner::Resident(candidate.unit_id),
        frame,
        state: [0; STATE_SIZE],
        exit: [0; EXIT_SIZE],
        extended: ExtendedCpuState::initial().unwrap(),
    });
    let usage = engine.cache_usage().unwrap();
    assert_eq!(
        engine.cancel_cache_request(KEY, candidate.serial),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(engine.pending_cache_request().unwrap(), Some(candidate));
    assert_eq!(engine.cache_usage().unwrap(), usage);
    assert!(
        engine
            .resident
            .as_ref()
            .unwrap()
            .get_retained_raw(engine.memory.as_ref().unwrap(), candidate.unit_id)
            .is_ok()
    );
    engine.pending_call = None;
    engine.cancel_cache_request(KEY, candidate.serial).unwrap();
    assert!(engine.pending_cache_request().unwrap().is_none());
    assert!(
        engine
            .resident
            .as_ref()
            .unwrap()
            .get_retained_raw(engine.memory.as_ref().unwrap(), candidate.unit_id)
            .is_err()
    );
}
