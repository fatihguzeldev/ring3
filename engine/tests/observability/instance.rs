use ring3_engine::{
    abi::arena::TRANSFER_OFFSET,
    process::{CallError, EngineInstance, ExecutionConfig32, HostError},
};
use ring3_observability::{Config, Level, Observer};

#[test]
fn instance_capture_precedes_game_start_and_survives_close_for_draining() {
    let observer = Observer::new(Config {
        level: Level::Detailed,
        ..Config::default()
    })
    .unwrap();
    let mut engine = EngineInstance::new(2, 7).unwrap();
    engine.set_observer(observer.clone()).unwrap();
    assert_eq!(
        engine.set_observer(Observer::disabled()),
        Err(HostError::Call(CallError::Busy))
    );
    engine.map(0x1000, 1, 7).unwrap();
    let code = [0xb8, 42, 0, 0, 0, 0xeb, 0xfe];
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + code.len()]
        .copy_from_slice(&code);
    engine.upload(0x1000, code.len() as u32).unwrap();
    engine
        .configure_execution(ExecutionConfig32 {
            units: 4,
            wasm_bytes: 1024 * 1024,
            promotion_hits: 256,
        })
        .unwrap();
    let request = engine.prepare_cache_entry(7, 0x1000).unwrap();
    assert_ne!(request.unit_id, 0);
    engine.close();
    assert!(engine.observer().stats().finished);
    let trace = observer.drain();
    for name in [
        "prepare_entry",
        "compile_cached",
        "emit_resident",
        "installation.requested",
        "capture.finished",
    ] {
        assert!(
            trace
                .windows(name.len())
                .any(|bytes| bytes == name.as_bytes()),
            "missing {name}"
        );
    }
    assert_eq!(observer.stats().dropped_events, 0);
}

#[test]
fn disabled_instance_does_not_inherit_another_components_capture() {
    let observer = Observer::new(Config {
        level: Level::Detailed,
        ..Config::default()
    })
    .unwrap();
    let _outer = observer.enter();
    let before = observer.stats().emitted_events;
    let mut engine = EngineInstance::new(4, 8).unwrap();
    assert!(engine.load_pe32(b"MZ").is_err());
    assert_eq!(observer.stats().emitted_events, before);
    assert!(ring3_engine::loader::load_pe32(b"MZ", 4).is_err());
    assert!(observer.stats().emitted_events > before);
}
