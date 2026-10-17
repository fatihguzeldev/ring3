use super::*;

fn invoke_fixture() -> EngineInstance {
    let mut engine = fixture();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x5118), &0x3130u32.to_le_bytes())
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x3130), &[0xb8, 0, 0, 0, 0, 0xc2, 36, 0])
        .unwrap();
    engine
}

fn invoke_request(engine: &mut EngineInstance) -> WindowsCallback32 {
    put(engine, 0x1400, Variant32::new(9, &0x5000u32.to_le_bytes()));
    put(engine, 0x1500, Variant32::EMPTY);
    let (_, _, _, outcome) = start(
        engine,
        Api::VariantChangeTypeEx,
        &[0x1500, 0x1400, 0x407, 0, 3],
    );
    let request = callback(outcome, 0x3130, 0x5000, 9);
    assert_eq!(request.arguments[1], 0);
    assert_eq!(read(engine, request.arguments[2], 16), [0; 16]);
    assert_eq!(request.arguments[3], 0x407);
    assert_eq!(request.arguments[4], 2);
    assert_eq!(read(engine, request.arguments[5], 16), [0; 16]);
    assert_eq!(read(engine, request.arguments[6], 16), Variant32::EMPTY.0);
    assert_eq!(&request.arguments[7..9], &[0, 0]);
    request
}

#[test]
fn captured_dispatch_default_property_uses_native_signature_and_publishes_its_numeric_value() {
    let mut engine = invoke_fixture();
    let request = invoke_request(&mut engine);
    let value = Variant32::new(3, &37i32.to_le_bytes());
    put(&mut engine, request.arguments[6], value);
    assert_eq!(
        engine
            .resume_automation_callback(request.continuation, 0)
            .unwrap(),
        Outcome::Return(0)
    );
    assert_eq!(read(&engine, 0x1500, 16), value.0);
    assert!(engine.automation.references.is_empty());
    assert!(engine.automation_process_scratch_ranges().next().is_none());
}

#[test]
fn captured_dispatch_failure_and_failed_conversion_release_the_owned_temporary_unknown_once() {
    for invoke_result in [0, 0x8002_0006] {
        let mut engine = invoke_fixture();
        let request = invoke_request(&mut engine);
        put(
            &mut engine,
            request.arguments[6],
            Variant32::new(13, &0x6000u32.to_le_bytes()),
        );
        let release = callback(
            engine
                .resume_automation_callback(request.continuation, invoke_result)
                .unwrap(),
            0x3120,
            0x6000,
            1,
        );
        assert_eq!(read(&engine, 0x1500, 16), Variant32::EMPTY.0);
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
            Outcome::Return(0x8002_0005)
        );
        assert_eq!(read(&engine, 0x1500, 16), Variant32::EMPTY.0);
        assert!(engine.automation.references.is_empty());
        assert!(engine.automation_process_scratch_ranges().next().is_none());
        assert_eq!(
            engine.resume_automation_callback(release.continuation, 0),
            Err(HostError::InvalidRequest)
        );
    }
}

fn returned_bstr(engine: &mut EngineInstance) -> u32 {
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1100), &[b'a', 0, 0, 0, b'z', 0])
        .unwrap();
    engine
        .automation_string(Api::SysAllocStringLen, &[0x1100, 3])
        .unwrap()
}

#[test]
fn owned_dispatch_bstr_result_publishes_embedded_null_text_and_retires_the_temporary_lifetime() {
    let mut engine = invoke_fixture();
    let temporary = returned_bstr(&mut engine);
    let identity = engine
        .windows_runtime_allocation_identity(temporary - 4)
        .unwrap();
    put(
        &mut engine,
        0x1400,
        Variant32::new(9, &0x5000u32.to_le_bytes()),
    );
    put(&mut engine, 0x1500, Variant32::EMPTY);
    let (_, _, _, outcome) = start(
        &mut engine,
        Api::VariantChangeTypeEx,
        &[0x1500, 0x1400, 0x407, 0, 8],
    );
    let invoke = callback(outcome, 0x3130, 0x5000, 9);
    put(
        &mut engine,
        invoke.arguments[6],
        Variant32::new(8, &temporary.to_le_bytes()),
    );
    assert_eq!(
        engine
            .resume_automation_callback(invoke.continuation, 0)
            .unwrap(),
        Outcome::Return(0)
    );
    let value = Variant32(read(&engine, 0x1500, 16).try_into().unwrap());
    assert_eq!(value.vartype(), 8);
    assert_eq!(
        read(&engine, value.pointer(), 8),
        [b'a', 0, 0, 0, b'z', 0, 0, 0]
    );
    assert_ne!(
        engine
            .windows_runtime_allocation_identity(value.pointer() - 4)
            .unwrap(),
        identity
    );
    assert_eq!(engine.automation.strings.len(), 1);
    assert_eq!(engine.automation.bytes, 12);
    assert!(engine.automation.references.is_empty());
    engine
        .automation_string(Api::SysFreeString, &[value.pointer()])
        .unwrap();
    assert_eq!(engine.automation.bytes, 0);
}

#[test]
fn temporary_retirement_projection_excludes_the_callers_old_destination_and_requires_no_ram_write()
{
    let mut engine = invoke_fixture();
    let old = returned_bstr(&mut engine);
    let temporary = returned_bstr(&mut engine);
    let identity = engine
        .windows_runtime_allocation_identity(temporary - 4)
        .unwrap();
    put(
        &mut engine,
        0x1400,
        Variant32::new(9, &0x5000u32.to_le_bytes()),
    );
    put(&mut engine, 0x1500, Variant32::new(8, &old.to_le_bytes()));
    let (_, _, _, outcome) = start(
        &mut engine,
        Api::VariantChangeTypeEx,
        &[0x1500, 0x1400, 0x407, 0, 8],
    );
    let invoke = callback(outcome, 0x3130, 0x5000, 9);
    put(
        &mut engine,
        invoke.arguments[6],
        Variant32::new(8, &temporary.to_le_bytes()),
    );
    engine
        .memory
        .as_mut()
        .unwrap()
        .protect(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::READ,
        )
        .unwrap();
    assert!(matches!(
        engine.resume_automation_callback(invoke.continuation, 0),
        Err(HostError::Memory(_))
    ));
    let rows = engine.automation_thread_temporary_allocations(1).unwrap();
    assert_eq!(rows, [(temporary - 4, identity)]);
    assert_eq!(
        engine.automation_process_temporary_allocations().unwrap(),
        rows
    );
    let version = engine.memory().unwrap().transaction_version();
    let retirement = engine
        .prepare_windows_runtime_retirement(&rows)
        .unwrap()
        .unwrap();
    engine.commit_windows_runtime_retirement_after_memory(Some(retirement));
    engine.commit_automation_temporary_retirement(&rows);
    engine.automation_retire_thread(1);
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    assert!(
        engine
            .windows_runtime_allocation_identity(temporary - 4)
            .is_err()
    );
    assert!(engine.windows_runtime_allocation_identity(old - 4).is_ok());
    assert_eq!(engine.automation.strings.len(), 1);
    assert_eq!(engine.automation.bytes, 12);
    assert_eq!(
        read(&engine, 0x1500, 16),
        Variant32::new(8, &old.to_le_bytes()).0
    );
}

#[test]
fn a_recycled_temporary_heap_block_is_rejected_without_freeing_the_replacement() {
    let mut engine = invoke_fixture();
    let temporary = returned_bstr(&mut engine);
    let identity = engine
        .windows_runtime_allocation_identity(temporary - 4)
        .unwrap();
    put(
        &mut engine,
        0x1400,
        Variant32::new(9, &0x5000u32.to_le_bytes()),
    );
    put(&mut engine, 0x1500, Variant32::EMPTY);
    let (_, _, _, outcome) = start(
        &mut engine,
        Api::VariantChangeTypeEx,
        &[0x1500, 0x1400, 0x407, 0, 8],
    );
    let invoke = callback(outcome, 0x3130, 0x5000, 9);
    put(
        &mut engine,
        invoke.arguments[6],
        Variant32::new(8, &temporary.to_le_bytes()),
    );
    let range = PageRange::new(GuestAddress(0x1000), 1).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .protect(range, Permissions::READ)
        .unwrap();
    assert!(matches!(
        engine.resume_automation_callback(invoke.continuation, 0),
        Err(HostError::Memory(_))
    ));
    engine
        .automation_string(Api::SysFreeString, &[temporary])
        .unwrap();
    let replacement = engine
        .automation_string(Api::SysAllocStringLen, &[0, 3])
        .unwrap();
    assert_eq!(replacement, temporary);
    let replacement_identity = engine
        .windows_runtime_allocation_identity(replacement - 4)
        .unwrap();
    assert_ne!(replacement_identity, identity);
    engine
        .memory
        .as_mut()
        .unwrap()
        .protect(range, Permissions::READ_WRITE)
        .unwrap();
    let version = engine.memory().unwrap().transaction_version();
    assert_eq!(
        engine.resume_automation_callback(invoke.continuation, 0),
        Err(HostError::Call(CallError::StateChanged))
    );
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    assert_eq!(
        engine
            .windows_runtime_allocation_identity(replacement - 4)
            .unwrap(),
        replacement_identity
    );
    assert_eq!(read(&engine, 0x1500, 16), Variant32::EMPTY.0);
    assert_eq!(engine.automation.strings.len(), 1);
}
