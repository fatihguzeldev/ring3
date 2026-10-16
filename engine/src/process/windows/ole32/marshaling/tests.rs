use super::*;
use crate::{
    memory::{GuestAddress, PageRange},
    process::{EngineInstance, HostError, windows::ole32::Ole32Config32},
    windows::{
        com::IID_IUNKNOWN,
        ole32::{ApartmentId32, IID_ICLASSFACTORY},
    },
};

fn engine() -> EngineInstance {
    let mut engine = EngineInstance::new(64, 77).unwrap();
    engine.map(0x10000, 2, 3).unwrap();
    engine.map(0x20000, 1, 5).unwrap();
    engine
        .configure_ole32(Ole32Config32 {
            objects: PageRange::new(GuestAddress(0x30000000), 16).unwrap(),
            max_objects: 8,
            max_threads: 8,
        })
        .unwrap();
    let runtime = engine.ole32.as_mut().unwrap();
    runtime.apartments.initialize(1, 0, 2).unwrap();
    runtime.methods = [GuestAddress(0x20000); 5];
    engine.write32(0x10100, 0x10200).unwrap();
    for slot in 0..5 {
        engine.write32(0x10200 + slot * 4, 0x20000).unwrap();
    }
    engine
}

#[test]
fn standard_proxy_publishes_an_apartment_bound_identity_without_the_server_pointer() {
    let mut engine = engine();
    let execution = ExecutionApartment32 {
        apartment: ApartmentId32::Neutral,
        thread: 1,
    };
    engine
        .publish_standard_com_proxy(
            execution,
            ApartmentId32::Sta(1),
            Some(7),
            0x10100,
            IID_ICLASSFACTORY,
            0x10300,
        )
        .unwrap();
    let runtime = engine.ole32.as_ref().unwrap();
    let proxy = runtime.marshaling.proxies[0].as_ref().unwrap();
    assert_ne!(proxy.factory_view.0, 0x10100);
    assert_ne!(proxy.unknown, proxy.factory_view);
    assert_eq!(
        runtime.com.lookup(proxy.unknown).unwrap(),
        runtime.com.lookup(proxy.factory_view).unwrap()
    );
    assert_eq!(
        runtime
            .com
            .query_target(engine.memory().unwrap(), proxy.factory_view, IID_IUNKNOWN)
            .unwrap(),
        proxy.unknown
    );
    assert_eq!(proxy.object, 0x10100);
    assert_eq!(proxy.factory, Some(0x10100));
    assert!(!proxy.factory_extra);
    assert_eq!(runtime.com.references(proxy.unknown).unwrap(), 1);
    assert_eq!(runtime.check_proxy_client(0, 1), Ok(()));
    assert!(runtime.check_proxy_client(0, 2).is_err());
}

#[test]
fn failed_proxy_publication_preserves_com_capacity_output_and_remote_reference_ownership() {
    let mut engine = engine();
    let before = engine.memory().unwrap().mapped_pages();
    let execution = ExecutionApartment32 {
        apartment: ApartmentId32::Neutral,
        thread: 1,
    };
    assert!(matches!(
        engine.publish_standard_com_proxy(
            execution,
            ApartmentId32::Sta(1),
            None,
            0x10100,
            IID_IUNKNOWN,
            0x50000
        ),
        Err(HostError::Memory(_))
    ));
    assert_eq!(engine.memory().unwrap().mapped_pages(), before);
    let runtime = engine.ole32.as_ref().unwrap();
    assert!(runtime.marshaling.proxies.iter().all(Option::is_none));
    assert_eq!(runtime.com.live_objects(), 0);
    engine
        .publish_standard_com_proxy(
            execution,
            ApartmentId32::Sta(1),
            None,
            0x10100,
            IID_IUNKNOWN,
            0x10300,
        )
        .unwrap();
    assert_eq!(engine.ole32.as_ref().unwrap().com.live_objects(), 1);
    assert_eq!(
        engine.ole32.as_ref().unwrap().marshaling.proxies[0]
            .as_ref()
            .unwrap()
            .object,
        0x10100
    );
}

#[path = "tests/lifetime.rs"]
mod lifetime;

#[path = "tests/rundown.rs"]
mod rundown;

#[path = "tests/local.rs"]
mod local;
