use super::super::tests::{call, engine, read};
use crate::{
    memory::GuestAddress,
    process::{EngineInstance, HostError, windows::callbacks::WindowsProviderOutcome32},
    windows::automation::{
        ArrayBound32, ArrayLayout32, AutomationError32 as E, Oleaut32Api32 as Api,
        variants::Variant32,
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
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x3100), &[0xb8, 1, 0, 0, 0, 0xc2, 4, 0])
        .unwrap();
    engine.map(0x5000, 1, 3).unwrap();
    write(&mut engine, 0x5000, &0x5100u32.to_le_bytes());
    write(&mut engine, 0x5104, &0x3100u32.to_le_bytes());
    write(&mut engine, 0x5108, &0x3100u32.to_le_bytes());
    engine
}
fn write(engine: &mut EngineInstance, at: u32, bytes: &[u8]) {
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(at), bytes)
        .unwrap();
}
fn word(engine: &EngineInstance, at: u32) -> u32 {
    u32::from_le_bytes(read(engine, at, 4).try_into().unwrap())
}
fn allocated(engine: &mut EngineInstance, kind: u16) -> u32 {
    let layout = ArrayLayout32::new(
        kind,
        &[ArrayBound32 { lower: 0, count: 1 }],
        true,
        engine.automation.limits,
    )
    .unwrap();
    engine
        .allocate_automation_array(layout, None, None)
        .unwrap()
}

#[test]
fn captured_interface_and_variant_arrays_have_native_features_zeroed_elements_and_retirement() {
    for (kind, features, size) in [(9, 0x2480, 4), (13, 0x2280, 4), (12, 0x2880, 16)] {
        let mut engine = fixture();
        let array = call(
            &mut engine,
            Api::SafeArrayCreateVector,
            &[kind, (-1i32) as u32, 3],
        );
        assert_ne!(array, 0);
        assert_eq!(read(&engine, array + 2, 2), (features as u16).to_le_bytes());
        assert_eq!(call(&mut engine, Api::SafeArrayGetElemsize, &[array]), size);
        let data = word(&engine, array + 12);
        assert_eq!(
            read(&engine, data, 3 * size as usize),
            vec![0; 3 * size as usize]
        );
        assert_eq!(call(&mut engine, Api::SafeArrayDestroy, &[array]), 0);
        assert_eq!(engine.automation.bytes, 0);
    }
}

#[test]
fn captured_unknown_put_uses_direct_pointer_and_holds_the_native_lock_during_addref() {
    let mut engine = fixture();
    let array = allocated(&mut engine, 13);
    write(&mut engine, 0x1200, &0u32.to_le_bytes());
    call(
        &mut engine,
        Api::SafeArrayPutElement,
        &[array, 0x1200, 0x5000],
    );
    let request = engine
        .pending_windows_callback()
        .expect("real captured PutElement must request guest AddRef");
    assert_eq!(request.arguments[0], 0x5000);
    assert_eq!(request.procedure.0, 0x3100);
    assert_eq!(request.count, 1);
    assert_eq!(word(&engine, array + 8), 1);
    assert_eq!(
        engine.automation_array_call(Api::SafeArrayDestroy, &[array]),
        Err(HostError::Automation(E::ArrayLocked))
    );
    assert_eq!(
        engine
            .resume_automation_callback(request.continuation, 2)
            .unwrap(),
        WindowsProviderOutcome32::Return(0)
    );
    assert_eq!(word(&engine, array + 8), 0);
    assert_eq!(word(&engine, word(&engine, array + 12)), 0x5000);
}

#[test]
fn captured_variant_get_deep_copies_into_uninitialized_output_without_clearing_its_garbage() {
    let mut engine = fixture();
    let array = allocated(&mut engine, 12);
    write(&mut engine, 0x1100, &[b'a', 0, 0, 0, b'z', 0]);
    let string = engine
        .automation_string(Api::SysAllocStringLen, &[0x1100, 3])
        .unwrap();
    let data = word(&engine, array + 12);
    write(
        &mut engine,
        data,
        &Variant32::new(8, &string.to_le_bytes()).0,
    );
    write(&mut engine, 0x1200, &0u32.to_le_bytes());
    write(&mut engine, 0x1400, &[0xa5; 16]);
    assert_eq!(
        call(
            &mut engine,
            Api::SafeArrayGetElement,
            &[array, 0x1200, 0x1400]
        ),
        0
    );
    let copy = Variant32(read(&engine, 0x1400, 16).try_into().unwrap());
    assert_eq!(copy.vartype(), 8);
    assert_ne!(copy.pointer(), string);
    assert_eq!(
        read(&engine, copy.pointer(), 8),
        [b'a', 0, 0, 0, b'z', 0, 0, 0]
    );
    assert_eq!(word(&engine, array + 8), 0);
    assert_eq!(engine.automation.strings.len(), 2);
}

fn resume(engine: &mut EngineInstance, expected: &[(u32, u32)]) {
    let mut request = engine
        .pending_windows_callback()
        .expect("captured array reference method");
    for (index, &(procedure, object)) in expected.iter().enumerate() {
        assert_eq!(
            (request.procedure.0, request.arguments[0], request.count),
            (procedure, object, 1)
        );
        let outcome = engine
            .resume_automation_callback(request.continuation, 1)
            .unwrap();
        if index + 1 == expected.len() {
            assert_eq!(outcome, WindowsProviderOutcome32::Return(0));
        } else {
            let WindowsProviderOutcome32::Callback(next) = outcome else {
                panic!("next real reference method");
            };
            request = next;
        }
    }
    assert!(engine.automation.references.is_empty());
}

#[test]
fn captured_interface_get_and_copy_addref_each_owned_pointer_and_publish_only_at_completion() {
    for copy in [false, true] {
        let mut engine = fixture();
        let array = allocated(&mut engine, 13);
        let data = word(&engine, array + 12);
        write(&mut engine, data, &0x5000u32.to_le_bytes());
        write(&mut engine, 0x1200, &0u32.to_le_bytes());
        write(&mut engine, 0x1400, &0xa5a5_a5a5u32.to_le_bytes());
        if copy {
            call(&mut engine, Api::SafeArrayCopy, &[array, 0x1400]);
        } else {
            call(
                &mut engine,
                Api::SafeArrayGetElement,
                &[array, 0x1200, 0x1400],
            );
        }
        assert_eq!(word(&engine, array + 8), 1);
        assert_eq!(word(&engine, 0x1400), 0xa5a5_a5a5);
        resume(&mut engine, &[(0x3100, 0x5000)]);
        assert_eq!(word(&engine, array + 8), 0);
        if copy {
            let cloned = word(&engine, 0x1400);
            assert_ne!(cloned, array);
            assert_eq!(word(&engine, word(&engine, cloned + 12)), 0x5000);
            assert_eq!(word(&engine, cloned + 8), 0);
            assert_eq!(engine.automation.arrays.len(), 2);
        } else {
            assert_eq!(word(&engine, 0x1400), 0x5000);
        }
    }
}

#[test]
fn captured_variant_array_destroy_releases_only_owned_interfaces_and_retires_nested_bstrs() {
    let mut engine = fixture();
    let layout = ArrayLayout32::new(
        12,
        &[ArrayBound32 { lower: 0, count: 3 }],
        true,
        engine.automation.limits,
    )
    .unwrap();
    let array = engine
        .allocate_automation_array(layout, None, None)
        .unwrap();
    write(&mut engine, 0x1100, &[b'a', 0, 0, 0, b'z', 0]);
    let string = engine
        .automation_string(Api::SysAllocStringLen, &[0x1100, 3])
        .unwrap();
    let data = word(&engine, array + 12);
    for (offset, value) in [
        (0, Variant32::new(13, &0x5000u32.to_le_bytes())),
        (16, Variant32::new(0x400d, &0x1600u32.to_le_bytes())),
        (32, Variant32::new(8, &string.to_le_bytes())),
    ] {
        write(&mut engine, data + offset, &value.0);
    }
    call(&mut engine, Api::SafeArrayDestroy, &[array]);
    assert_eq!(
        engine.automation_array_call(Api::SafeArrayAccessData, &[array, 0x1400]),
        Err(HostError::Automation(E::ArrayLocked))
    );
    resume(&mut engine, &[(0x3100, 0x5000)]);
    assert!(engine.automation.arrays.is_empty());
    assert!(engine.automation.strings.is_empty());
    assert_eq!(engine.automation.bytes, 0);
}

#[test]
fn captured_interface_redim_transfers_prefix_without_addref_and_releases_only_the_removed_suffix() {
    let mut engine = fixture();
    let layout = ArrayLayout32::new(
        13,
        &[ArrayBound32 {
            lower: -2,
            count: 2,
        }],
        true,
        engine.automation.limits,
    )
    .unwrap();
    let array = engine
        .allocate_automation_array(layout, None, None)
        .unwrap();
    let data = word(&engine, array + 12);
    write(
        &mut engine,
        data,
        &[0x5000u32.to_le_bytes(), 0x5000u32.to_le_bytes()].concat(),
    );
    write(
        &mut engine,
        0x1200,
        &[1u32.to_le_bytes(), 7u32.to_le_bytes()].concat(),
    );
    call(&mut engine, Api::SafeArrayRedim, &[array, 0x1200]);
    assert_eq!(word(&engine, array + 16), 2);
    resume(&mut engine, &[(0x3100, 0x5000)]);
    assert_eq!(word(&engine, array + 16), 1);
    assert_eq!(word(&engine, array + 20), 7);
    assert_eq!(word(&engine, word(&engine, array + 12)), 0x5000);
    assert_eq!(engine.automation.arrays.len(), 1);
}

#[test]
fn accepted_array_addref_keeps_its_lock_on_write_fault_and_retry_does_not_repeat_the_method() {
    use crate::memory::{PageRange, Permissions};
    let mut engine = fixture();
    engine.map(0x7000, 1, 3).unwrap();
    let array = allocated(&mut engine, 13);
    let data = word(&engine, array + 12);
    write(&mut engine, data, &0x5000u32.to_le_bytes());
    write(&mut engine, 0x1200, &0u32.to_le_bytes());
    write(&mut engine, 0x7000, &0xa5a5_a5a5u32.to_le_bytes());
    call(
        &mut engine,
        Api::SafeArrayGetElement,
        &[array, 0x1200, 0x7000],
    );
    let request = engine.pending_windows_callback().unwrap();
    let range = PageRange::new(GuestAddress(0x7000), 1).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .protect(range, Permissions::READ)
        .unwrap();
    let version = engine.memory().unwrap().transaction_version();
    assert!(matches!(
        engine.resume_automation_callback(request.continuation, 2),
        Err(HostError::Memory(_))
    ));
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    assert_eq!(word(&engine, array + 8), 1);
    assert_eq!(word(&engine, 0x7000), 0xa5a5_a5a5);
    engine
        .memory
        .as_mut()
        .unwrap()
        .protect(range, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(
        engine
            .resume_automation_callback(request.continuation, 2)
            .unwrap(),
        WindowsProviderOutcome32::Return(0)
    );
    assert_eq!(word(&engine, array + 8), 0);
    assert_eq!(word(&engine, 0x7000), 0x5000);
    assert!(engine.automation.references.is_empty());
}

#[test]
fn aggregate_array_retirement_preserves_a_caller_lock_and_is_neutral_before_ram_acceptance() {
    let mut engine = fixture();
    let array = allocated(&mut engine, 13);
    write(&mut engine, 0x1200, &0u32.to_le_bytes());
    assert_eq!(
        engine
            .automation_array_call(Api::SafeArrayLock, &[array])
            .unwrap(),
        0
    );
    call(
        &mut engine,
        Api::SafeArrayPutElement,
        &[array, 0x1200, 0x5000],
    );
    assert_eq!(word(&engine, array + 8), 2);
    let thread = engine.current_windows_thread_id().unwrap();
    let version = engine.memory().unwrap().transaction_version();
    let receipt = engine
        .prepare_automation_array_retirement(Some(thread))
        .unwrap();
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    assert_eq!(word(&engine, array + 8), 2);
    assert_eq!(receipt.words().len(), 1);
    assert_eq!(receipt.words()[0].value, 1);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write_words32(receipt.words())
        .unwrap();
    engine.commit_automation_array_retirement(receipt);
    engine.automation_retire_thread(thread);
    assert_eq!(word(&engine, array + 8), 1);
    assert_eq!(
        engine
            .automation_array_call(Api::SafeArrayUnlock, &[array])
            .unwrap(),
        0
    );
    assert_eq!(word(&engine, array + 8), 0);
}
