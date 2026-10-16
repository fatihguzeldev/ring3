use super::context::{NativeReference32, TransferCall32, TransferParent32, TransferPhase32};
use crate::{
    cpu::x86::State32,
    memory::{GuestAddress, PageRange},
    process::{
        EngineInstance, HostError,
        calls::call::PendingOwner,
        windows::{
            callbacks::GuestCallbackScratch32,
            ole32::{
                Ole32Config32,
                activation::{ActivationRuntime32, PendingActivation32, Phase32},
                marshaling::{ExecutionApartment32, Remote32, RemoteActivation32},
            },
        },
    },
    windows::{
        CallFrame32, CallingConvention32,
        com::{InterfaceId, ServerThreadingModel32},
        ole32::{ApartmentId32, Ole32Api32},
    },
};
fn engine(source: ApartmentId32, client: ApartmentId32) -> (EngineInstance, u32, u32) {
    let mut engine = EngineInstance::new(64, 7).unwrap();
    engine.map(0x10000, 2, 3).unwrap();
    engine.map(0x20000, 1, 5).unwrap();
    engine
        .configure_ole32(Ole32Config32 {
            objects: PageRange::new(GuestAddress(0x30000000), 16).unwrap(),
            max_objects: 8,
            max_threads: 8,
        })
        .unwrap();
    engine
        .ole32
        .as_mut()
        .unwrap()
        .apartments
        .initialize(1, 0, if client == ApartmentId32::Mta { 0 } else { 2 })
        .unwrap();
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
    let parent_scratch = GuestCallbackScratch32::allocate(
        engine.memory.as_mut().unwrap(),
        ActivationRuntime32::scratch_reservation(),
        &[0; 64],
    )
    .unwrap();
    let scratch = GuestCallbackScratch32::allocate(
        engine.memory.as_mut().unwrap(),
        ActivationRuntime32::scratch_reservation(),
        &[0; 128],
    )
    .unwrap();
    let execution = ExecutionApartment32 {
        apartment: source,
        thread: match source {
            ApartmentId32::Sta(thread) => thread,
            _ => 2,
        },
    };
    let client = ExecutionApartment32 {
        apartment: client,
        thread: 1,
    };
    let mut refs = [NativeReference32 {
        pointer: 0,
        execution,
    }; 4];
    refs[0].pointer = 0x10100;
    refs[1].pointer = 0x10100;
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
        scratch: parent_scratch,
        phase: Phase32::Factory,
        threading: ServerThreadingModel32::Apartment,
        server_threads: vec![1],
        server: None,
    });
    runtime.marshaling.activations.push(RemoteActivation32 {
        continuation: 1,
        client: client.apartment,
        execution,
        remote: Remote32::Ready(0),
        next: None,
        abandoned: false,
        cleanup: None,
    });
    runtime.marshaling.transfers.push(TransferCall32 {
        continuation: 2,
        parent: TransferParent32::Activation(1),
        owner: PendingOwner::Replacement(1),
        token: 1,
        frame,
        sender: 1,
        requested: InterfaceId([0x55; 16]),
        source: execution,
        client,
        execution,
        scratch: Some(scratch),
        remote: Remote32::Idle,
        phase: TransferPhase32::Class,
        references: refs,
        stream: None,
        packet: false,
        result: 0,
        output: 0,
        abandoned: false,
        cancelled: false,
        dead_sta: [0; 2],
    });
    (engine, parent_scratch.address.0, scratch.address.0)
}
#[test]
fn exact_client_mta_invocation_is_eligible_without_opening_unrelated_mta_reentry() {
    let (mut engine, _, _) = engine(ApartmentId32::Sta(2), ApartmentId32::Mta);
    let call = &mut engine.ole32.as_mut().unwrap().marshaling.transfers[0];
    call.phase = TransferPhase32::Activate;
    call.execution = call.client;
    call.remote = Remote32::Queued;
    assert!(engine.com_apartment_invocation_eligible(1));
    engine.ole32.as_mut().unwrap().marshaling.transfers[0].sender = 3;
    assert!(!engine.com_apartment_invocation_eligible(1));
}

#[test]
fn ready_transfer_rejects_remapped_scratch_before_acquiring_a_pointer_or_selecting_registered_ps() {
    for result in [0, 0x80004002] {
        let (mut engine, parent, scratch) = engine(ApartmentId32::Sta(2), ApartmentId32::Mta);
        let call = &mut engine.ole32.as_mut().unwrap().marshaling.transfers[0];
        call.phase = TransferPhase32::QueryMarshal;
        call.references[1].pointer = 0;
        call.remote = Remote32::Ready(result);
        engine.unmap(scratch, 1).unwrap();
        engine.map(scratch, 1, 3).unwrap();
        engine.write32(scratch + 48, 0xdeadbeef).unwrap();
        let version = engine.memory().unwrap().transaction_version();
        assert!(matches!(
            engine.advance_com_transfer(0),
            Err(HostError::Memory(_))
        ));
        let runtime = engine.ole32.as_ref().unwrap();
        let call = &runtime.marshaling.transfers[0];
        assert!(call.phase == TransferPhase32::QueryMarshal);
        assert_eq!(call.references[1].pointer, 0);
        assert_eq!(call.remote, Remote32::Ready(result));
        assert_eq!(runtime.rpc.live_brokers(), 0);
        assert_eq!(engine.memory().unwrap().transaction_version(), version);
        assert_eq!(engine.ole32_read_word(scratch + 48).unwrap(), 0xdeadbeef);
        assert_eq!(runtime.activation.pending[0].scratch.address.0, parent);
    }
}
#[test]
fn sender_exit_escrows_both_parent_and_transfer_scratch_and_duplicate_native_references() {
    let (mut engine, parent, scratch) = engine(ApartmentId32::Sta(2), ApartmentId32::Sta(1));
    let before = engine.memory().unwrap().transaction_version();
    let prepared = engine.prepare_com_thread_exit(1).unwrap();
    assert!(prepared.ranges().is_empty());
    assert_eq!(engine.memory().unwrap().transaction_version(), before);
    engine.commit_com_thread_exit(prepared);
    let runtime = engine.ole32.as_ref().unwrap();
    let call = &runtime.marshaling.transfers[0];
    assert!(call.abandoned && call.cancelled);
    assert_eq!(call.references[0].pointer, 0x10100);
    assert_eq!(call.references[1].pointer, 0x10100);
    assert_eq!(runtime.activation.pending.len(), 1);
    assert_eq!(runtime.marshaling.activations.len(), 1);
    assert!(
        engine
            .memory()
            .unwrap()
            .permissions_at(GuestAddress(parent))
            .is_ok()
    );
    assert!(
        engine
            .memory()
            .unwrap()
            .permissions_at(GuestAddress(scratch))
            .is_ok()
    );
    assert!(engine.com_abandoned_work_ready(2));
}
#[test]
fn dead_source_sta_drops_only_its_apartment_references_and_keeps_the_callers_result_lease() {
    let (mut engine, parent, _) = engine(ApartmentId32::Sta(2), ApartmentId32::Mta);
    let call = &mut engine.ole32.as_mut().unwrap().marshaling.transfers[0];
    call.references[2] = NativeReference32 {
        pointer: 0x10200,
        execution: call.client,
    };
    call.phase = TransferPhase32::Unmarshal;
    call.execution = call.client;
    call.remote = Remote32::Queued;
    call.packet = true;
    let prepared = engine.prepare_com_thread_exit(2).unwrap();
    assert!(prepared.ranges().is_empty());
    engine.commit_com_thread_exit(prepared);
    let call = &engine.ole32.as_ref().unwrap().marshaling.transfers[0];
    assert!(call.cancelled && !call.abandoned && !call.packet);
    assert_eq!(call.references[0].pointer, 0);
    assert_eq!(call.references[1].pointer, 0);
    assert_eq!(call.references[2].pointer, 0x10200);
    assert!(
        engine
            .memory()
            .unwrap()
            .permissions_at(GuestAddress(parent))
            .is_ok()
    );
}
#[test]
fn stale_transfer_scratch_rejects_aggregate_teardown_without_adopting_a_replacement() {
    let (mut engine, _, scratch) = engine(ApartmentId32::Sta(2), ApartmentId32::Sta(1));
    engine.unmap(scratch, 1).unwrap();
    engine.map(scratch, 1, 3).unwrap();
    let before = engine.memory().unwrap().transaction_version();
    assert!(matches!(
        engine.prepare_com_thread_exit(1),
        Err(HostError::Memory(_))
    ));
    assert_eq!(engine.memory().unwrap().transaction_version(), before);
    let call = &engine.ole32.as_ref().unwrap().marshaling.transfers[0];
    assert!(!call.abandoned && !call.cancelled);
    assert_eq!(call.references[0].pointer, 0x10100);
}
#[test]
fn ready_marshal_after_source_sta_death_does_not_resurrect_a_dead_marshaler_packet() {
    let (mut engine, parent, _) = engine(ApartmentId32::Sta(2), ApartmentId32::Mta);
    let gates =
        crate::windows::ole32::StreamMethod32::ALL.map(|method| crate::cpu::dbt::GateSpec {
            entry: GuestAddress(0x20000),
            id: method.id(),
        });
    engine
        .ole32
        .as_mut()
        .unwrap()
        .initialize_stream_methods(&gates)
        .unwrap();
    let stream = engine
        .ole32
        .as_mut()
        .unwrap()
        .create_stream(engine.memory.as_mut().unwrap(), 8, b"12345678")
        .unwrap();
    let runtime = engine.ole32.as_mut().unwrap();
    let call = &mut runtime.marshaling.transfers[0];
    call.phase = TransferPhase32::Marshal;
    call.remote = Remote32::Ready(0);
    call.stream = Some(stream);
    let prepared = engine.prepare_com_thread_exit(2).unwrap();
    engine.commit_com_thread_exit(prepared);
    engine.accept_com_transfer_result(0, 0).unwrap();
    let runtime = engine.ole32.as_ref().unwrap();
    let call = &runtime.marshaling.transfers[0];
    assert!(call.phase == TransferPhase32::Complete && !call.packet);
    assert_eq!(call.result, 0x8001_0108);
    assert!(call.stream.is_none() && call.scratch.is_none());
    assert_eq!(runtime.streams.store.reserved_bytes(), 0);
    assert!(runtime.com.lookup(stream).is_err());
    assert!(
        engine
            .memory()
            .unwrap()
            .permissions_at(GuestAddress(parent))
            .is_ok()
    );
    assert_eq!(runtime.activation.pending.len(), 1);
}
#[test]
fn retained_transfer_identity_rejects_a_changed_parent_token_before_invocation_entry() {
    let (mut engine, _, _) = engine(ApartmentId32::Sta(2), ApartmentId32::Mta);
    let call = &mut engine.ole32.as_mut().unwrap().marshaling.transfers[0];
    call.phase = TransferPhase32::Activate;
    call.execution = call.client;
    call.remote = Remote32::Queued;
    engine.ole32.as_mut().unwrap().activation.pending[0].token = 3;
    let before = engine.memory().unwrap().transaction_version();
    assert!(matches!(
        engine.prepare_com_apartment_entry(1, 2),
        Err(HostError::InvalidRequest)
    ));
    assert_eq!(engine.memory().unwrap().transaction_version(), before);
    assert!(engine.ole32.as_ref().unwrap().marshaling.transfers[0].remote == Remote32::Queued);
}
