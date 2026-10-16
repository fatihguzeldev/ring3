use crate::{
    abi::{
        arena::{EXIT_OFFSET, TRANSFER_OFFSET},
        x86::{EXIT_SIZE, STATE_SIZE, encode_exit_v3, encode_state_current},
    },
    cpu::{ExecutionExit, ExitReason, x86::State32},
    memory::{GuestAddress, PageRange, Permissions},
    process::windows::callbacks::{
        WindowsCallback32, WindowsCallbackProvider32 as Provider,
        WindowsProviderOutcome32 as Outcome,
    },
    process::{
        CallError, EngineInstance, HostError,
        calls::call::PendingOwner,
        windows::automation::tests::{engine, read},
    },
    windows::{
        CallFrame32,
        automation::{Oleaut32Api32 as Api, variants::Variant32},
    },
};

fn fixture() -> EngineInstance {
    let mut engine = engine();
    engine.map(0x3000, 1, 7).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x3000), &[0x0f, 0x0b])
        .unwrap();
    for page in [0x5000, 0x6000] {
        engine.map(page, 1, 3).unwrap();
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(page), &(page + 0x100u32).to_le_bytes())
            .unwrap();
        for (slot, procedure) in [(0, 0x3100u32), (1, 0x3110), (2, 0x3120)] {
            engine
                .memory
                .as_mut()
                .unwrap()
                .write(
                    GuestAddress(page + 0x100 + slot * 4),
                    &procedure.to_le_bytes(),
                )
                .unwrap();
        }
    }
    for (at, words) in [(0x3100, 12), (0x3110, 4), (0x3120, 4)] {
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(at), &[0xb8, 1, 0, 0, 0, 0xc2, words, 0])
            .unwrap();
    }
    engine
}

fn put(engine: &mut EngineInstance, address: u32, value: Variant32) {
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(address), &value.0)
        .unwrap();
}

fn start(
    engine: &mut EngineInstance,
    api: Api,
    arguments: &[u32],
) -> (PendingOwner, u32, CallFrame32, Outcome) {
    for (index, value) in [0x3000u32, 2, 0x3000, api.id()].into_iter().enumerate() {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&value.to_le_bytes());
    }
    engine.compile_with_gates(1, 1).unwrap();
    for (index, value) in std::iter::once(0x3002)
        .chain(arguments.iter().copied())
        .enumerate()
    {
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(
                GuestAddress(0x1000 + index as u32 * 4),
                &value.to_le_bytes(),
            )
            .unwrap();
    }
    let mut state = State32 {
        eip: 0x3000,
        ..Default::default()
    };
    state.registers[4] = 0x1000;
    encode_state_current(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate { id: api.id() },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    let generation = engine.generation;
    let token = engine
        .capture_windows_call(0xa070, generation)
        .unwrap()
        .token;
    let owner = PendingOwner::Replacement(generation);
    let frame = CallFrame32::capture(
        engine.memory().unwrap(),
        state,
        api.convention(),
        api.stack_words(),
    )
    .unwrap();
    let outcome = engine
        .automation_variant_outcome(owner, token, frame, api)
        .unwrap();
    (owner, token, frame, outcome)
}

fn callback(outcome: Outcome, procedure: u32, object: u32, count: u32) -> WindowsCallback32 {
    let Outcome::Callback(request) = outcome else {
        panic!("expected actual provider callback request");
    };
    assert_eq!(request.provider, Provider::Automation);
    assert_eq!(request.procedure.0, procedure);
    assert_eq!(request.arguments[0], object);
    assert_eq!(request.count, count);
    request
}

#[test]
fn captured_reference_copy_requests_addref_before_old_release_and_final_release_may_unmap_object() {
    let mut engine = fixture();
    let source = Variant32::new(13, &0x5000u32.to_le_bytes());
    let old = Variant32::new(9, &0x6000u32.to_le_bytes());
    put(&mut engine, 0x1400, source);
    put(&mut engine, 0x1500, old);
    let (owner, token, frame, outcome) = start(&mut engine, Api::VariantCopy, &[0x1500, 0x1400]);
    let add = callback(outcome, 0x3110, 0x5000, 1);
    assert_eq!(read(&engine, 0x1500, 16), old.0);
    engine
        .check_automation_callback(owner, token, frame, add.continuation)
        .unwrap();
    let version = engine.memory().unwrap().transaction_version();
    assert_eq!(
        engine.check_automation_callback(owner, token + 1, frame, add.continuation),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    let release = callback(
        engine
            .resume_automation_callback(add.continuation, 2)
            .unwrap(),
        0x3120,
        0x6000,
        1,
    );
    assert_eq!(read(&engine, 0x1500, 16), old.0);
    engine
        .check_automation_callback(owner, token, frame, release.continuation)
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .unmap_ranges_with_word_writes(&[PageRange::new(GuestAddress(0x6000), 1).unwrap()], &[])
        .unwrap();
    assert_eq!(
        engine
            .resume_automation_callback(release.continuation, 0)
            .unwrap(),
        Outcome::Return(0)
    );
    assert_eq!(read(&engine, 0x1500, 16), source.0);
    assert!(engine.automation.references.is_empty());
    assert_eq!(
        engine.resume_automation_callback(release.continuation, 0),
        Err(HostError::InvalidRequest)
    );
}

#[test]
fn captured_query_transfers_one_returned_reference_and_preserves_failure_hresult_without_addref_replay()
 {
    for result in [0, 0x8000_4002] {
        let mut engine = fixture();
        put(
            &mut engine,
            0x1400,
            Variant32::new(13, &0x5000u32.to_le_bytes()),
        );
        let old = Variant32::new(3, &37i32.to_le_bytes());
        put(&mut engine, 0x1500, old);
        let (owner, token, frame, outcome) = start(
            &mut engine,
            Api::VariantChangeTypeEx,
            &[0x1500, 0x1400, 0x407, 0, 9],
        );
        let query = callback(outcome, 0x3100, 0x5000, 3);
        assert_eq!(
            read(&engine, query.arguments[1], 16),
            [0, 4, 2, 0, 0, 0, 0, 0, 0xc0, 0, 0, 0, 0, 0, 0, 0x46]
        );
        assert_eq!(read(&engine, query.arguments[2], 4), [0; 4]);
        engine
            .check_automation_callback(owner, token, frame, query.continuation)
            .unwrap();
        if result == 0 {
            engine
                .memory
                .as_mut()
                .unwrap()
                .write(GuestAddress(query.arguments[2]), &0x6000u32.to_le_bytes())
                .unwrap();
        }
        assert_eq!(
            engine
                .resume_automation_callback(query.continuation, result)
                .unwrap(),
            Outcome::Return(result)
        );
        assert_eq!(
            read(&engine, 0x1500, 16),
            if result == 0 {
                Variant32::new(9, &0x6000u32.to_le_bytes()).0
            } else {
                old.0
            }
        );
        assert!(engine.automation.references.is_empty());
        assert!(engine.automation_process_scratch_ranges().next().is_none());
        assert!(
            engine
                .memory()
                .unwrap()
                .permissions_at(GuestAddress(query.arguments[1]))
                .is_err()
        );
    }
}

#[test]
fn nested_output_replacement_requests_compensating_release_without_overwriting_new_value() {
    let mut engine = fixture();
    put(
        &mut engine,
        0x1400,
        Variant32::new(13, &0x5000u32.to_le_bytes()),
    );
    put(&mut engine, 0x1500, Variant32::EMPTY);
    let (_, _, _, outcome) = start(&mut engine, Api::VariantCopy, &[0x1500, 0x1400]);
    let add = callback(outcome, 0x3110, 0x5000, 1);
    let nested = Variant32::new(3, &41i32.to_le_bytes());
    put(&mut engine, 0x1500, nested);
    let release = callback(
        engine
            .resume_automation_callback(add.continuation, 2)
            .unwrap(),
        0x3120,
        0x5000,
        1,
    );
    assert_eq!(read(&engine, 0x1500, 16), nested.0);
    assert!(matches!(
        engine.resume_automation_callback(release.continuation, 1),
        Err(HostError::Call(CallError::StateChanged))
    ));
    assert_eq!(read(&engine, 0x1500, 16), nested.0);
}

#[test]
fn accepted_addref_survives_output_rights_repair_without_replaying_the_guest_method() {
    let mut engine = fixture();
    let source = Variant32::new(13, &0x5000u32.to_le_bytes());
    put(&mut engine, 0x1400, source);
    put(&mut engine, 0x1500, Variant32::EMPTY);
    let (_, _, _, outcome) = start(&mut engine, Api::VariantCopy, &[0x1500, 0x1400]);
    let add = callback(outcome, 0x3110, 0x5000, 1);
    let range = PageRange::new(GuestAddress(0x1000), 1).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .protect(range, Permissions::READ)
        .unwrap();
    assert!(matches!(
        engine.resume_automation_callback(add.continuation, 2),
        Err(HostError::Memory(_))
    ));
    assert_eq!(read(&engine, 0x1500, 16), Variant32::EMPTY.0);
    engine
        .memory
        .as_mut()
        .unwrap()
        .protect(range, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(
        engine
            .resume_automation_callback(add.continuation, 2)
            .unwrap(),
        Outcome::Return(0)
    );
    assert_eq!(read(&engine, 0x1500, 16), source.0);
    assert!(engine.automation.references.is_empty());
}

#[test]
fn changed_query_destination_compensates_the_returned_reference_and_accepts_final_unmapping() {
    let mut engine = fixture();
    put(
        &mut engine,
        0x1400,
        Variant32::new(13, &0x5000u32.to_le_bytes()),
    );
    put(&mut engine, 0x1500, Variant32::EMPTY);
    let (owner, token, frame, outcome) = start(
        &mut engine,
        Api::VariantChangeTypeEx,
        &[0x1500, 0x1400, 0x407, 0, 9],
    );
    let query = callback(outcome, 0x3100, 0x5000, 3);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(query.arguments[2]), &0x6000u32.to_le_bytes())
        .unwrap();
    let nested = Variant32::new(3, &41i32.to_le_bytes());
    put(&mut engine, 0x1500, nested);
    let release = callback(
        engine
            .resume_automation_callback(query.continuation, 0)
            .unwrap(),
        0x3120,
        0x6000,
        1,
    );
    engine
        .check_automation_callback(owner, token, frame, release.continuation)
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .unmap_ranges_with_word_writes(&[PageRange::new(GuestAddress(0x6000), 1).unwrap()], &[])
        .unwrap();
    let expected = Err(HostError::Call(CallError::StateChanged));
    assert_eq!(
        engine.resume_automation_callback(release.continuation, 0),
        expected
    );
    assert_eq!(
        engine.resume_automation_callback(release.continuation, 0),
        expected
    );
    assert_eq!(read(&engine, 0x1500, 16), nested.0);
    assert!(engine.automation_process_scratch_ranges().next().is_none());
}

#[test]
fn compensation_never_calls_a_replacement_object_at_the_same_guest_address() {
    let mut engine = fixture();
    put(
        &mut engine,
        0x1400,
        Variant32::new(13, &0x5000u32.to_le_bytes()),
    );
    put(&mut engine, 0x1500, Variant32::EMPTY);
    let (_, _, _, outcome) = start(&mut engine, Api::VariantCopy, &[0x1500, 0x1400]);
    let add = callback(outcome, 0x3110, 0x5000, 1);
    let range = PageRange::new(GuestAddress(0x5000), 1).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .unmap_ranges_with_word_writes(&[range], &[])
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .map_zeroed(range, Permissions::READ_WRITE)
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x5000), &0x5100u32.to_le_bytes())
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x5108), &0x3120u32.to_le_bytes())
        .unwrap();
    let nested = Variant32::new(3, &41i32.to_le_bytes());
    put(&mut engine, 0x1500, nested);
    let version = engine.memory().unwrap().transaction_version();
    assert_eq!(
        engine.resume_automation_callback(add.continuation, 2),
        Err(HostError::Call(CallError::StateChanged))
    );
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    assert_eq!(read(&engine, 0x1500, 16), nested.0);
}
