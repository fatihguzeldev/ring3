use super::*;

#[test]
fn source_death_after_own_request_free_retires_only_its_replacement_packet() {
    source_death_after_request_page_reuse(Some(12), true);
}

#[test]
fn source_death_after_own_request_free_does_not_adopt_an_unsubmitted_reused_page() {
    source_death_after_request_page_reuse(None, true);
}

#[test]
fn source_death_after_own_request_free_does_not_restore_its_new_response_at_the_same_address() {
    source_death_after_request_page_reuse(None, false);
}

fn source_death_after_request_page_reuse(other_owner: Option<u64>, other_first: bool) {
    let mut engine = engine();
    let channel = channel(&mut engine, 1, 0);
    let (scratch, original) = invoke(&mut engine, Remote32::Queued);
    let request = buffers::read_message(engine.memory().unwrap(), 0x10000).unwrap();
    let (replacement, other, other_bytes) = {
        let runtime = engine.ole32.as_mut().unwrap();
        let memory = engine.memory.as_mut().unwrap();
        runtime
            .free_rpc_buffer_for(memory, 0, scratch, Some(11))
            .unwrap();
        assert_eq!(runtime.rpc.calls[0].request.unwrap(), request);

        message(memory, 0x10040, 12);
        memory
            .write(GuestAddress(scratch), &request.encode())
            .unwrap();
        let addresses = if other_first {
            [0x10040, scratch]
        } else {
            [scratch, 0x10040]
        };
        for address in addresses {
            runtime.allocate_rpc_buffer(memory, 0, address).unwrap();
            runtime.rpc.buffers.last_mut().unwrap().owner = if address == scratch {
                Some(11)
            } else {
                other_owner
            };
        }
        let replacement = runtime.rpc.buffers[usize::from(other_first)].scratch;
        let other = runtime.rpc.buffers[usize::from(!other_first)].scratch;
        assert_eq!(
            if other_first {
                other.address.0
            } else {
                replacement.address.0
            },
            original,
            "the first subsequent packet reuses the freed page"
        );
        let other_bytes = [0x12, 0x34, 0x56, 0x78];
        memory.write(other.address, &other_bytes).unwrap();
        memory.write(replacement.address, &[0xab; 12]).unwrap();
        (replacement, other, other_bytes)
    };

    let exit = engine.prepare_com_thread_exit(2).unwrap();
    assert!(exit.ranges().is_empty());
    engine.check_com_thread_exit(&exit).unwrap();
    engine.commit_com_thread_exit_after_memory(exit, false);
    let call = &engine.ole32.as_ref().unwrap().rpc.calls[0];
    assert!(call.cancelled && !call.abandoned);
    assert_eq!(call.remote, Remote32::Ready(0x8001_0108));
    let version = engine.memory().unwrap().transaction_version();

    engine.advance_rpc_call(0).unwrap();

    let runtime = engine.ole32.as_ref().unwrap();
    assert!(runtime.rpc.calls.is_empty());
    assert_eq!(runtime.rpc.buffers.len(), 1);
    assert_eq!(runtime.rpc.buffers[0].owner, other_owner);
    assert_eq!(runtime.streams.store.reserved_bytes(), 4096);
    assert_eq!(runtime.com.references(channel).unwrap(), 1);
    runtime
        .com
        .preflight(engine.memory().unwrap(), channel)
        .unwrap();
    assert_eq!(
        buffers::read_message(engine.memory().unwrap(), 0x10000).unwrap(),
        crate::windows::ole32::rpc::RpcMessage32 {
            buffer: 0,
            length: 0,
            ..request
        }
    );
    assert_eq!(
        crate::abi::x86::decode_state(&engine.arena()[..crate::abi::x86::STATE_SIZE])
            .unwrap()
            .registers[0],
        0x8001_0108
    );
    assert_eq!(engine.memory().unwrap().transaction_version(), version + 1);
    assert!(
        engine
            .memory()
            .unwrap()
            .permissions_at(GuestAddress(scratch))
            .is_err()
    );
    assert!(replacement.check(engine.memory().unwrap()).is_err());
    other.check(engine.memory().unwrap()).unwrap();
    let mut actual = [0; 4];
    engine
        .memory()
        .unwrap()
        .read(other.address, &mut actual)
        .unwrap();
    assert_eq!(actual, other_bytes);
}
