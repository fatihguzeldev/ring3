use super::*;

#[test]
fn ready_rpc_query_rejects_a_replacement_page_before_adopting_its_output_pointer() {
    let mut engine = engine();
    let (scratch, _) = invoke(&mut engine, Remote32::Ready(0));
    engine.ole32.as_mut().unwrap().rpc.calls[0].phase = RpcPhase32::Query(InterfaceId([9; 16]));
    let range = PageRange::new(GuestAddress(scratch), 1).unwrap();
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
    engine.write32(scratch + 16, 0xdeadbeef).unwrap();
    let version = engine.memory().unwrap().transaction_version();
    assert!(matches!(
        engine.advance_rpc_call(0),
        Err(HostError::Memory(_))
    ));
    let runtime = engine.ole32.as_ref().unwrap();
    assert!(runtime.marshaling.transfers.is_empty());
    assert!(runtime.rpc.calls[0].phase == RpcPhase32::Query(InterfaceId([9; 16])));
    assert_eq!(runtime.rpc.calls[0].remote, Remote32::Ready(0));
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    assert_eq!(engine.ole32_read_word(scratch + 16).unwrap(), 0xdeadbeef);
}

#[test]
fn accepted_packet_cannot_be_reassigned_or_freed_by_a_different_nested_call() {
    let mut engine = engine();
    let (_, buffer) = invoke(&mut engine, Remote32::Queued);
    let version = engine.memory().unwrap().transaction_version();
    let runtime = engine.ole32.as_mut().unwrap();
    let memory = engine.memory.as_mut().unwrap();
    assert_eq!(
        runtime.rpc.packet(0, buffer, 12, None),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(
        runtime.rpc.packet(0, buffer, 12, Some(12)),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(
        runtime.free_rpc_buffer_for(memory, 0, 0x10000, Some(12)),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(runtime.rpc.buffers[0].owner, Some(11));
    assert_eq!(memory.transaction_version(), version);
    runtime.rpc.buffers[0].scratch.check(memory).unwrap();
    assert_eq!(
        buffers::read_message(memory, 0x10000).unwrap().buffer,
        buffer
    );
    runtime
        .free_rpc_buffer_for(memory, 0, 0x10000, Some(11))
        .unwrap();
    assert!(runtime.rpc.buffers.is_empty());
    assert_eq!(buffers::read_message(memory, 0x10000).unwrap().buffer, 0);
    assert_eq!(runtime.streams.store.reserved_bytes(), 0);
}
