use super::super::context::{Broker32, BuildPhase32, RpcCall32, RpcPhase32};
use super::*;
use crate::{
    cpu::x86::State32,
    process::{
        EngineInstance,
        calls::call::PendingOwner,
        windows::{
            callbacks::GuestCallbackScratch32,
            ole32::{
                activation::ActivationRuntime32,
                marshaling::{ExecutionApartment32, Remote32},
            },
        },
    },
    windows::{CallFrame32, CallingConvention32, com::InterfaceId, ole32::ApartmentId32},
};
fn engine() -> EngineInstance {
    let (runtime, memory) = fixture();
    let mut engine = EngineInstance::new(16, 7).unwrap();
    engine.ole32 = Some(runtime);
    engine.memory = Some(memory);
    let runtime = engine.ole32.as_mut().unwrap();
    runtime.apartments.initialize(1, 0, 0).unwrap();
    runtime.apartments.initialize(3, 0, 0).unwrap();
    runtime.rpc.brokers[0] = Some(Broker32 {
        source: ExecutionApartment32 {
            apartment: ApartmentId32::Sta(2),
            thread: 2,
        },
        client: ExecutionApartment32 {
            apartment: ApartmentId32::Mta,
            thread: 1,
        },
        requested: InterfaceId([9; 16]),
        source_object: 0x10100,
        owns_source: true,
        result: 0,
        source_factory: 0,
        stub: 0x10200,
        client_factory: 0,
        proxy: 0x10300,
        interface: 0x10304,
        controller: None,
        channel: None,
        controller_live: false,
        channel_live: false,
        proxy_channel_references: 0,
        connected: true,
        build: BuildPhase32::Publish,
        rundown: None,
    });
    engine
}
fn invoke(engine: &mut EngineInstance, remote: Remote32) -> (u32, u32) {
    let runtime = engine.ole32.as_mut().unwrap();
    let memory = engine.memory.as_mut().unwrap();
    message(memory, 0x10000, 12);
    runtime.allocate_rpc_buffer(memory, 0, 0x10000).unwrap();
    runtime.rpc.buffers[0].owner = Some(11);
    let request = buffers::read_message(memory, 0x10000).unwrap();
    let request_lease = runtime.rpc.buffers[0].id;
    let held_channel = runtime.rpc.brokers[0].as_ref().unwrap().channel;
    if let Some(channel) = held_channel {
        let receipt = runtime
            .com
            .prepare_lifetime_changes(memory, &[], &[], &[channel], &[], &[])
            .unwrap();
        runtime
            .com
            .commit_lifetime_changes(memory, receipt)
            .unwrap();
    }
    let scratch = GuestCallbackScratch32::allocate(
        memory,
        ActivationRuntime32::scratch_reservation(),
        &request.encode(),
    )
    .unwrap();
    let mut state = State32::default();
    state.registers[4] = 0x10800;
    let frame = CallFrame32::capture(memory, state, CallingConvention32::Stdcall, 3).unwrap();
    let address = scratch.address.0;
    runtime.rpc.calls.push(RpcCall32 {
        continuation: 11,
        sender: 1,
        owner: PendingOwner::Replacement(7),
        token: 19,
        frame,
        broker: 0,
        execution: ExecutionApartment32 {
            apartment: ApartmentId32::Sta(2),
            thread: 2,
        },
        remote,
        phase: RpcPhase32::Invoke,
        message: 0x10000,
        status: 0,
        scratch: Some(scratch),
        request_buffer: request.buffer,
        abandoned: false,
        cancelled: false,
        returned: 0,
        request: Some(request),
        request_lease: Some(request_lease),
        held_channel,
    });
    (address, request.buffer)
}
#[test]
fn source_sta_death_invalidates_only_its_native_end_and_retains_the_client_packet() {
    let mut engine = engine();
    let (scratch, buffer) = invoke(&mut engine, Remote32::Queued);
    let before = engine.memory().unwrap().transaction_version();
    let prepared = engine.prepare_com_thread_exit(2).unwrap();
    assert!(prepared.ranges().is_empty());
    assert_eq!(engine.memory().unwrap().transaction_version(), before);
    engine.commit_com_thread_exit(prepared);
    let runtime = engine.ole32.as_ref().unwrap();
    let broker = runtime.rpc.brokers[0].as_ref().unwrap();
    assert_eq!(
        (broker.stub, broker.source_object, broker.proxy),
        (0, 0, 0x10300)
    );
    assert!(!broker.connected);
    let call = &runtime.rpc.calls[0];
    assert_eq!(call.remote, Remote32::Ready(0x80010108));
    assert!(call.cancelled && !call.abandoned);
    assert_eq!(call.request.unwrap().buffer, buffer);
    call.scratch
        .unwrap()
        .check(engine.memory().unwrap())
        .unwrap();
    runtime.rpc.buffers[0]
        .scratch
        .check(engine.memory().unwrap())
        .unwrap();
    assert_eq!(call.scratch.unwrap().address.0, scratch);
}
#[test]
fn dead_sender_retires_its_packet_and_scratch_without_consuming_another_calls_buffer_or_cpu() {
    let mut engine = engine();
    let (scratch, buffer) = invoke(&mut engine, Remote32::Ready(0));
    {
        let runtime = engine.ole32.as_mut().unwrap();
        let memory = engine.memory.as_mut().unwrap();
        message(memory, 0x10040, 1);
        runtime.allocate_rpc_buffer(memory, 0, 0x10040).unwrap();
        runtime.rpc.buffers[1].owner = Some(12);
    }
    let other = engine.ole32.as_ref().unwrap().rpc.buffers[1]
        .scratch
        .address;
    let arena = engine.arena.as_ref().get_ref().to_vec();
    let prepared = engine.prepare_com_thread_exit(1).unwrap();
    assert!(prepared.ranges().is_empty());
    engine.commit_com_thread_exit(prepared);
    assert!(engine.abandoned_rpc_work_ready());
    engine.service_abandoned_rpc_work().unwrap();
    let runtime = engine.ole32.as_ref().unwrap();
    assert!(runtime.rpc.calls.is_empty());
    assert_eq!(runtime.rpc.buffers.len(), 1);
    assert_eq!(runtime.rpc.buffers[0].owner, Some(12));
    assert_eq!(runtime.streams.store.reserved_bytes(), 4096);
    assert!(
        engine
            .memory()
            .unwrap()
            .permissions_at(GuestAddress(scratch))
            .is_err()
    );
    assert!(
        engine
            .memory()
            .unwrap()
            .permissions_at(GuestAddress(buffer))
            .is_err()
    );
    assert_eq!(
        engine.memory().unwrap().permissions_at(other),
        Ok(Permissions::READ_WRITE)
    );
    assert_eq!(engine.arena.as_ref().get_ref(), arena.as_slice());
}
#[test]
fn stale_rpc_packet_aborts_aggregate_thread_exit_before_disconnection_or_reference_retirement() {
    let mut engine = engine();
    let (_, buffer) = invoke(&mut engine, Remote32::Ready(0));
    let range = PageRange::new(GuestAddress(buffer), 1).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .unmap_ranges(&[range])
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .map_zeroed(range, Permissions::READ_WRITE)
        .unwrap();
    let before = engine.memory().unwrap().transaction_version();
    assert!(matches!(
        engine.prepare_com_thread_exit(1),
        Err(HostError::Memory(MemoryError::InvalidRange))
    ));
    assert_eq!(engine.memory().unwrap().transaction_version(), before);
    let runtime = engine.ole32.as_ref().unwrap();
    assert!(!runtime.rpc.calls[0].abandoned);
    assert!(runtime.rpc.brokers[0].as_ref().unwrap().connected);
    assert_eq!(runtime.streams.store.reserved_bytes(), 4096);
}
#[test]
fn registered_proxy_rundown_waits_for_the_last_mta_member_and_uses_the_shared_issuer() {
    let mut engine = engine();
    engine.ole32.as_mut().unwrap().activation.next = 41;
    let before = engine.memory().unwrap().transaction_version();
    let prepared = engine.prepare_com_rundown(1, false, 41).unwrap();
    engine.commit_com_rundown(prepared);
    assert!(!engine.rpc_rundown_pending(1));
    engine.ole32.as_mut().unwrap().apartments.uninitialize(3);
    let prepared = engine.prepare_com_rundown(1, false, 41).unwrap();
    assert!(
        engine.ole32.as_ref().unwrap().rpc.brokers[0]
            .as_ref()
            .unwrap()
            .rundown
            .is_none()
    );
    engine.commit_com_rundown(prepared);
    let runtime = engine.ole32.as_ref().unwrap();
    let broker = runtime.rpc.brokers[0].as_ref().unwrap();
    assert!(!broker.connected);
    let rundown = broker.rundown.unwrap();
    assert_eq!(
        (rundown.continuation, rundown.sender, rundown.next),
        (42, 1, 0)
    );
    assert_eq!(runtime.activation.next, 42);
    assert!(engine.rpc_rundown_pending(1));
    assert_eq!(engine.memory().unwrap().transaction_version(), before);
}
#[test]
fn exhausted_registered_proxy_rundown_preserves_connected_identity_and_native_refs() {
    let mut engine = engine();
    let runtime = engine.ole32.as_mut().unwrap();
    runtime.apartments.uninitialize(3);
    runtime.activation.next = u64::MAX;
    let before = engine.memory().unwrap().transaction_version();
    assert!(matches!(
        engine.prepare_com_rundown(1, false, u64::MAX),
        Err(HostError::Infrastructure)
    ));
    let broker = engine.ole32.as_ref().unwrap().rpc.brokers[0]
        .as_ref()
        .unwrap();
    assert!(broker.connected && broker.rundown.is_none());
    assert_eq!(
        (broker.stub, broker.proxy, broker.source_object),
        (0x10200, 0x10300, 0x10100)
    );
    assert_eq!(engine.memory().unwrap().transaction_version(), before);
}

fn channel(engine: &mut EngineInstance, native: u32, independent: u32) -> GuestAddress {
    use crate::{process::windows::objects::InterfaceLayout32, windows::com::IID_IUNKNOWN};
    let runtime = engine.ole32.as_mut().unwrap();
    runtime.apartments.uninitialize(1);
    runtime.apartments.initialize(1, 0, 2).unwrap();
    let memory = engine.memory.as_mut().unwrap();
    let identity = runtime
        .com
        .create(
            memory,
            super::super::context::CHANNEL_PAYLOAD,
            &[InterfaceLayout32 {
                iid: IID_IUNKNOWN,
                methods: &[GuestAddress(0x10000); 3],
            }],
        )
        .unwrap();
    let pointer = identity.identity_pointer;
    for _ in 1..native + independent {
        runtime.com.add_ref(memory, pointer).unwrap();
    }
    let broker = runtime.rpc.brokers[0].as_mut().unwrap();
    broker.client = ExecutionApartment32 {
        apartment: ApartmentId32::Sta(1),
        thread: 1,
    };
    broker.channel = Some(pointer);
    broker.channel_live = true;
    broker.proxy_channel_references = native;
    pointer
}

#[test]
fn client_sta_channel_retirement_joins_one_aggregate_ram_and_word_transaction() {
    let mut engine = engine();
    let pointer = channel(&mut engine, 1, 0);
    let common = PageRange::new(GuestAddress(0x20000), 1).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .map_zeroed(common, Permissions::READ_WRITE)
        .unwrap();
    let before = engine.memory().unwrap().transaction_version();
    let prepared = engine.prepare_com_thread_exit(1).unwrap();
    assert_eq!(prepared.ranges(), &[PageRange::new(pointer, 1).unwrap()]);
    engine.check_com_thread_exit(&prepared).unwrap();
    let mut ranges = prepared.ranges().to_vec();
    ranges.push(common);
    ranges.sort_unstable_by_key(|range| range.first);
    engine
        .memory
        .as_mut()
        .unwrap()
        .unmap_ranges_with_word_writes(
            &ranges,
            &[crate::memory::WordWrite32 {
                address: GuestAddress(0x10000),
                value: 0xabcdef01,
            }],
        )
        .unwrap();
    engine.commit_com_thread_exit_after_memory(prepared, true);
    assert_eq!(engine.memory().unwrap().transaction_version(), before + 1);
    let runtime = engine.ole32.as_ref().unwrap();
    assert!(runtime.com.lookup(pointer).is_err());
    let broker = runtime.rpc.brokers[0].as_ref().unwrap();
    assert!(!broker.channel_live && !broker.connected);
    assert_eq!(broker.proxy_channel_references, 0);
    assert_eq!((broker.proxy, broker.stub), (0, 0x10200));
    assert!(engine.memory().unwrap().permissions_at(pointer).is_err());
    assert!(
        engine
            .memory()
            .unwrap()
            .permissions_at(GuestAddress(0x20000))
            .is_err()
    );
    assert_eq!(engine.ole32_read_word(0x10000).unwrap(), 0xabcdef01);
}

#[test]
fn client_sta_retires_only_observed_native_channel_refs_and_preserves_other_holders_without_ram_mutation()
 {
    let mut engine = engine();
    let pointer = channel(&mut engine, 2, 1);
    let before = engine.memory().unwrap().transaction_version();
    let prepared = engine.prepare_com_thread_exit(1).unwrap();
    assert!(prepared.ranges().is_empty());
    engine.check_com_thread_exit(&prepared).unwrap();
    engine.commit_com_thread_exit_after_memory(prepared, false);
    assert_eq!(engine.memory().unwrap().transaction_version(), before);
    let runtime = engine.ole32.as_ref().unwrap();
    assert_eq!(runtime.com.references(pointer).unwrap(), 1);
    runtime
        .com
        .preflight(engine.memory().unwrap(), pointer)
        .unwrap();
    assert!(runtime.rpc.brokers[0].as_ref().unwrap().channel_live);
    assert_eq!(
        runtime.rpc.brokers[0]
            .as_ref()
            .unwrap()
            .proxy_channel_references,
        0
    );
}

#[test]
fn stale_channel_mapping_rejects_thread_exit_before_native_disconnect_or_counter_changes() {
    let mut engine = engine();
    let pointer = channel(&mut engine, 1, 0);
    let range = PageRange::new(pointer, 1).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .unmap_ranges(&[range])
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .map_zeroed(range, Permissions::READ)
        .unwrap();
    let before = engine.memory().unwrap().transaction_version();
    assert!(engine.prepare_com_thread_exit(1).is_err());
    assert_eq!(engine.memory().unwrap().transaction_version(), before);
    let runtime = engine.ole32.as_ref().unwrap();
    let broker = runtime.rpc.brokers[0].as_ref().unwrap();
    assert!(broker.connected && broker.channel_live);
    assert_eq!(broker.proxy_channel_references, 1);
    assert_eq!(runtime.com.references(pointer).unwrap(), 1);
}

#[test]
fn exhausted_channel_retirement_preserves_native_refs_until_one_atomic_retry() {
    let mut engine = engine();
    let pointer = channel(&mut engine, 1, 0);
    let version = engine.memory().unwrap().transaction_version();
    engine.memory.as_mut().unwrap().exhaust_versions_for_test();
    assert!(matches!(
        engine.prepare_com_thread_exit(1),
        Err(HostError::Memory(MemoryError::VersionExhausted))
    ));
    let runtime = engine.ole32.as_ref().unwrap();
    assert!(runtime.rpc.brokers[0].as_ref().unwrap().connected);
    assert_eq!(runtime.com.references(pointer).unwrap(), 1);
    engine
        .memory
        .as_mut()
        .unwrap()
        .restore_version_for_test(version);
    let prepared = engine.prepare_com_thread_exit(1).unwrap();
    engine.check_com_thread_exit(&prepared).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .unmap_ranges_with_word_writes(prepared.ranges(), &[])
        .unwrap();
    engine.commit_com_thread_exit_after_memory(prepared, true);
    assert!(engine.ole32.as_ref().unwrap().com.lookup(pointer).is_err());
    assert_eq!(engine.memory().unwrap().transaction_version(), version + 1);
}

#[test]
fn source_response_cannot_steal_a_nested_calls_packet_or_consume_either_lease() {
    let mut engine = engine();
    let (scratch, original) = invoke(&mut engine, Remote32::Ready(0));
    {
        let runtime = engine.ole32.as_mut().unwrap();
        let memory = engine.memory.as_mut().unwrap();
        message(memory, 0x10040, 4);
        runtime.allocate_rpc_buffer(memory, 0, 0x10040).unwrap();
        runtime.rpc.buffers[1].owner = Some(12);
        let response = buffers::read_message(memory, 0x10040).unwrap();
        memory
            .write(GuestAddress(scratch), &response.encode())
            .unwrap();
    }
    let version = engine.memory().unwrap().transaction_version();
    let arena = engine.arena.as_ref().get_ref().to_vec();
    assert_eq!(engine.advance_rpc_call(0), Err(HostError::InvalidRequest));
    let runtime = engine.ole32.as_ref().unwrap();
    assert_eq!(runtime.rpc.calls.len(), 1);
    assert_eq!(runtime.rpc.buffers.len(), 2);
    assert_eq!(runtime.rpc.buffers[0].owner, Some(11));
    assert_eq!(runtime.rpc.buffers[1].owner, Some(12));
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    assert_eq!(
        buffers::read_message(engine.memory().unwrap(), 0x10000)
            .unwrap()
            .buffer,
        original
    );
    assert_eq!(engine.arena.as_ref().get_ref(), arena.as_slice());
}

#[test]
fn abandoned_query_release_failure_keeps_its_native_reference_and_retry_phase() {
    let mut engine = engine();
    let (scratch, _) = invoke(&mut engine, Remote32::Idle);
    let call = &mut engine.ole32.as_mut().unwrap().rpc.calls[0];
    call.phase = RpcPhase32::ReleaseQuery;
    call.returned = 0xdead0000;
    call.abandoned = true;
    let version = engine.memory().unwrap().transaction_version();
    assert!(engine.service_abandoned_rpc_work().is_err());
    let runtime = engine.ole32.as_ref().unwrap();
    let call = &runtime.rpc.calls[0];
    assert!(call.phase == RpcPhase32::ReleaseQuery);
    assert_eq!(call.returned, 0xdead0000);
    assert_eq!(runtime.rpc.buffers.len(), 1);
    assert_eq!(call.scratch.unwrap().address.0, scratch);
    call.scratch
        .unwrap()
        .check(engine.memory().unwrap())
        .unwrap();
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
}

mod packet;
mod ready;
