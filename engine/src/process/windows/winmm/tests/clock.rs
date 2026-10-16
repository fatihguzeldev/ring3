use crate::process::EngineInstance;
#[test]
fn multimedia_time_uses_the_shared_clock_and_wraps_at_exact_dword_boundary() {
    let mut engine = EngineInstance::new(8, 7).unwrap();
    engine.configure_cpu_clock(10_000_000, 123_000_000).unwrap();
    assert_eq!(engine.multimedia_milliseconds().unwrap(), 0);
    engine
        .observe_cpu_clock(123_000_000 + ((u64::from(u32::MAX) + 1) * 1_000_000) + 9_000_000)
        .unwrap();
    assert_eq!(engine.multimedia_milliseconds().unwrap(), 9);
    assert_eq!(
        engine.cpu_clock.timestamp().unwrap(),
        (u64::from(u32::MAX) + 10) * 10_000
    );
}
