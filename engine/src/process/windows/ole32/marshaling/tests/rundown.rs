use super::*;

fn proxy(engine: &mut EngineInstance, client: ApartmentId32) {
    engine
        .publish_standard_com_proxy(
            ExecutionApartment32 {
                apartment: ApartmentId32::Neutral,
                thread: 1,
            },
            client,
            None,
            0x10100,
            IID_ICLASSFACTORY,
            0x10300,
        )
        .unwrap();
}

#[test]
fn apartment_rundown_preflights_without_guest_mutation_and_preserves_two_identical_native_references()
 {
    let mut engine = engine();
    proxy(&mut engine, ApartmentId32::Sta(1));
    let runtime = engine.ole32.as_mut().unwrap();
    runtime.activation.next = 41;
    runtime.marshaling.proxies[0].as_mut().unwrap().owned[1] = 0x10100;
    let before = engine.memory().unwrap().transaction_version();
    let prepared = engine.prepare_com_rundown(1, false, 41).unwrap();
    let entry = engine.ole32.as_ref().unwrap().marshaling.proxies[0]
        .as_ref()
        .unwrap();
    assert!(!entry.disconnected);
    assert!(entry.rundown.is_none());
    assert_eq!(engine.memory().unwrap().transaction_version(), before);
    engine.commit_com_rundown(prepared);
    let runtime = engine.ole32.as_ref().unwrap();
    let entry = runtime.marshaling.proxies[0].as_ref().unwrap();
    assert!(entry.disconnected);
    assert_eq!(entry.owned, [0x10100, 0x10100]);
    let rundown = entry.rundown.unwrap();
    assert_eq!(
        (
            rundown.sender,
            rundown.continuation,
            rundown.next,
            rundown.remote
        ),
        (1, 42, 0, Remote32::Idle)
    );
    assert_eq!(runtime.com.references(entry.pointer).unwrap(), 1);
    assert_eq!(engine.memory().unwrap().transaction_version(), before);
    assert!(engine.com_rundown_pending(1));
    assert!(engine.com_abandoned_work_ready(1));
}

#[test]
fn mta_proxy_rundown_waits_for_the_last_user_apartment_member() {
    let mut engine = engine();
    engine
        .ole32
        .as_mut()
        .unwrap()
        .apartments
        .initialize(2, 0, 0)
        .unwrap();
    engine
        .ole32
        .as_mut()
        .unwrap()
        .apartments
        .initialize(3, 0, 0)
        .unwrap();
    proxy(&mut engine, ApartmentId32::Mta);
    let prepared = engine.prepare_com_rundown(2, false, 0).unwrap();
    engine.commit_com_rundown(prepared);
    assert!(!engine.com_rundown_pending(2));
    assert!(
        !engine.ole32.as_ref().unwrap().marshaling.proxies[0]
            .as_ref()
            .unwrap()
            .disconnected
    );
    engine.ole32.as_mut().unwrap().apartments.uninitialize(3);
    let prepared = engine.prepare_com_rundown(2, false, 0).unwrap();
    engine.commit_com_rundown(prepared);
    assert!(engine.com_rundown_pending(2));
}

#[test]
fn exhausted_rundown_issuer_preserves_apartment_proxy_and_guest_memory() {
    let mut engine = engine();
    proxy(&mut engine, ApartmentId32::Sta(1));
    engine.ole32.as_mut().unwrap().activation.next = u64::MAX;
    let before = engine.memory().unwrap().transaction_version();
    assert!(matches!(
        engine.prepare_com_rundown(1, false, u64::MAX),
        Err(HostError::Infrastructure)
    ));
    let entry = engine.ole32.as_ref().unwrap().marshaling.proxies[0]
        .as_ref()
        .unwrap();
    assert!(!entry.disconnected);
    assert!(entry.rundown.is_none());
    assert_eq!(entry.owned, [0x10100, 0]);
    assert_eq!(engine.memory().unwrap().transaction_version(), before);
}

#[test]
fn an_mta_thread_can_switch_to_its_own_neutral_context_without_accepting_unrelated_mta_reentry() {
    let mut engine = engine();
    engine.ole32.as_mut().unwrap().apartments.uninitialize(1);
    engine
        .ole32
        .as_mut()
        .unwrap()
        .apartments
        .initialize(1, 0, 0)
        .unwrap();
    super::lifetime::pending(
        &mut engine,
        ExecutionApartment32 {
            apartment: ApartmentId32::Neutral,
            thread: 1,
        },
    );
    assert!(engine.com_apartment_invocation_eligible(1));
    engine.ole32.as_mut().unwrap().marshaling.activations[0]
        .execution
        .apartment = ApartmentId32::Mta;
    assert!(!engine.com_apartment_invocation_eligible(1));
    engine.ole32.as_mut().unwrap().marshaling.activations[0]
        .execution
        .apartment = ApartmentId32::Neutral;
    engine.ole32.as_mut().unwrap().activation.pending[0].thread = 2;
    assert!(!engine.com_apartment_invocation_eligible(1));
}
