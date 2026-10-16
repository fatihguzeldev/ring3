use super::*;
use crate::{
    cpu::x86::State32,
    process::{
        calls::call::PendingOwner,
        windows::{
            callbacks::GuestCallbackScratch32,
            ole32::activation::{ActivationRuntime32, PendingActivation32, Phase32},
        },
    },
    windows::{CallFrame32, CallingConvention32, com::ServerThreadingModel32, ole32::Ole32Api32},
};

pub(super) fn pending(engine: &mut EngineInstance, execution: ExecutionApartment32) -> u32 {
    let mut state = State32 {
        eip: 0x20000,
        ..Default::default()
    };
    state.registers[4] = 0x10800;
    engine.write32(0x10800, 0x20010).unwrap();
    let frame = CallFrame32::capture(
        engine.memory().unwrap(),
        state,
        CallingConvention32::Stdcall,
        5,
    )
    .unwrap();
    let scratch = GuestCallbackScratch32::allocate(
        engine.memory.as_mut().unwrap(),
        ActivationRuntime32::scratch_reservation(),
        &[0; 64],
    )
    .unwrap();
    let address = scratch.address.0;
    let runtime = engine.ole32.as_mut().unwrap();
    runtime.activation.pending.push(PendingActivation32 {
        continuation: 1,
        thread: 1,
        owner: PendingOwner::Replacement(1),
        token: 1,
        frame,
        api: Ole32Api32::CoGetClassObject,
        out: 0x10300,
        outer: 0,
        path: "native.dll".into(),
        scratch,
        phase: Phase32::Factory,
        threading: ServerThreadingModel32::Apartment,
        server_threads: vec![1],
        server: None,
    });
    runtime.marshaling.activations.push(RemoteActivation32 {
        continuation: 1,
        client: ApartmentId32::Sta(1),
        execution,
        remote: Remote32::Queued,
        next: None,
        abandoned: false,
        cleanup: None,
    });
    address
}

#[test]
fn canceled_queued_activation_retires_only_its_exact_scratch_after_aggregate_commit() {
    let mut engine = engine();
    let address = pending(
        &mut engine,
        ExecutionApartment32 {
            apartment: ApartmentId32::Sta(2),
            thread: 2,
        },
    );
    let prepared = engine.prepare_com_thread_exit(1).unwrap();
    assert_eq!(
        prepared.ranges(),
        &[PageRange::new(GuestAddress(address), 1).unwrap()]
    );
    let before = engine.memory().unwrap().transaction_version();
    assert_eq!(engine.ole32.as_ref().unwrap().activation.pending.len(), 1);
    assert_eq!(engine.memory().unwrap().transaction_version(), before);
    engine
        .memory
        .as_mut()
        .unwrap()
        .unmap_ranges_with_word_writes(prepared.ranges(), &[])
        .unwrap();
    engine.commit_com_thread_exit(prepared);
    assert!(engine.ole32.as_ref().unwrap().activation.pending.is_empty());
    assert!(
        engine
            .ole32
            .as_ref()
            .unwrap()
            .marshaling
            .activations
            .is_empty()
    );
    assert!(
        engine
            .memory()
            .unwrap()
            .permissions_at(GuestAddress(address))
            .is_err()
    );
}

#[test]
fn ready_activation_escrows_the_returned_reference_and_preserves_duplicate_reference_counts() {
    let mut engine = engine();
    let address = pending(
        &mut engine,
        ExecutionApartment32 {
            apartment: ApartmentId32::Sta(2),
            thread: 2,
        },
    );
    engine.write32(address + 52, 0x10100).unwrap();
    engine.ole32.as_mut().unwrap().marshaling.activations[0].remote = Remote32::Ready(0);
    let prepared = engine.prepare_com_thread_exit(1).unwrap();
    assert!(prepared.ranges().is_empty());
    engine.commit_com_thread_exit(prepared);
    let runtime = engine.ole32.as_ref().unwrap();
    let remote = &runtime.marshaling.activations[0];
    assert!(remote.abandoned);
    let cleanup = remote.cleanup.unwrap();
    assert_eq!(
        (cleanup.pointers, cleanup.count, cleanup.next),
        ([0x10100, 0], 1, 0)
    );
    runtime.activation.pending[0]
        .scratch
        .check(engine.memory().unwrap())
        .unwrap();
    assert!(engine.com_abandoned_work_ready(2));
    assert!(!engine.com_abandoned_work_ready(1));
    let duplicate = super::super::context::Cleanup32::new([0x10100, 0x10100]);
    assert_eq!(
        (duplicate.pointers, duplicate.count),
        ([0x10100, 0x10100], 2)
    );
}

#[test]
fn target_termination_disconnects_the_live_caller_without_reclaiming_its_output_lease() {
    let mut engine = engine();
    let address = pending(
        &mut engine,
        ExecutionApartment32 {
            apartment: ApartmentId32::Sta(2),
            thread: 2,
        },
    );
    engine.write32(0x10300, 0x12345678).unwrap();
    let prepared = engine.prepare_com_thread_exit(2).unwrap();
    assert!(prepared.ranges().is_empty());
    engine.commit_com_thread_exit(prepared);
    let runtime = engine.ole32.as_ref().unwrap();
    assert_eq!(
        runtime.activation.pending[0].phase,
        Phase32::Finish { result: 0x80010108 }
    );
    assert_eq!(runtime.marshaling.activations[0].remote, Remote32::Idle);
    runtime.activation.pending[0]
        .scratch
        .check(engine.memory().unwrap())
        .unwrap();
    assert_eq!(engine.ole32_read_word(0x10300).unwrap(), 0x12345678);
    assert_eq!(engine.ole32_read_word(address + 52).unwrap(), 0);
}

#[test]
fn stale_scratch_prevents_thread_exit_preparation_without_adopting_replacement_memory() {
    let mut engine = engine();
    let address = pending(
        &mut engine,
        ExecutionApartment32 {
            apartment: ApartmentId32::Sta(2),
            thread: 2,
        },
    );
    engine.unmap(address, 1).unwrap();
    engine.map(address, 1, 3).unwrap();
    engine.write32(address, 0x12345678).unwrap();
    let before = engine.memory().unwrap().transaction_version();
    assert!(matches!(
        engine.prepare_com_thread_exit(1),
        Err(HostError::Memory(_))
    ));
    assert_eq!(engine.memory().unwrap().transaction_version(), before);
    assert_eq!(engine.ole32_read_word(address).unwrap(), 0x12345678);
    assert!(!engine.ole32.as_ref().unwrap().marshaling.activations[0].abandoned);
}

#[test]
fn accepted_remote_queue_publishes_scheduler_marker_after_exact_target_validation() {
    let mut engine = engine();
    pending(
        &mut engine,
        ExecutionApartment32 {
            apartment: ApartmentId32::Neutral,
            thread: 1,
        },
    );
    assert_eq!(engine.pending_windows_thread_schedule(), None);
    let before = engine.memory().unwrap().transaction_version();
    assert!(
        engine
            .queue_com_apartment_invocation(2, GuestAddress(0x20000), &[0x10100], 1, 1)
            .is_err()
    );
    assert_eq!(engine.pending_windows_thread_schedule(), None);
    assert_eq!(engine.memory().unwrap().transaction_version(), before);
    engine
        .queue_com_apartment_invocation(1, GuestAddress(0x20000), &[0x10100], 1, 1)
        .unwrap();
    assert_eq!(engine.pending_windows_thread_schedule(), Some(1));
    assert!(engine.has_com_apartment_invocation(1));
    assert_eq!(engine.memory().unwrap().transaction_version(), before);
}
