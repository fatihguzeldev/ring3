use super::*;

#[test]
fn samples_share_a_fixed_epoch_and_frequency_without_read_advancement() {
    let mut clock = CpuClock::default();
    clock.configure(2_500_000_000, 8_000).unwrap();
    assert_eq!(clock.timestamp(), Ok(0));
    clock.observe(8_101).unwrap();
    for _ in 0..3 {
        assert_eq!(clock.timestamp(), Ok(252));
    }
    clock.observe(8_101).unwrap();
    assert_eq!(clock.timestamp(), Ok(252));
    clock.observe(8_102).unwrap();
    assert_eq!(clock.timestamp(), Ok(255));
}

#[test]
fn fractional_ticks_are_retained_across_observations() {
    let mut clock = CpuClock::default();
    clock.configure(3, 0).unwrap();
    for now in [1, 333_333_333] {
        clock.observe(now).unwrap();
        assert_eq!(clock.timestamp(), Ok(0));
    }
    clock.observe(333_333_334).unwrap();
    assert_eq!(clock.timestamp(), Ok(1));
    clock.observe(1_000_000_000).unwrap();
    assert_eq!(clock.timestamp(), Ok(3));
}

#[test]
fn invalid_configuration_and_regressing_samples_leave_the_clock_unchanged() {
    let mut clock = CpuClock::default();
    assert_eq!(clock.timestamp(), Err(HostError::Infrastructure));
    assert_eq!(clock.observe(0), Err(HostError::Infrastructure));
    assert_eq!(clock.advance(0), Err(HostError::Infrastructure));
    assert_eq!(clock.configure(0, 100), Err(HostError::InvalidRequest));
    clock.configure(1_000_000_000, 100).unwrap();
    clock.observe(117).unwrap();
    for now in [0, 99, 100, 116] {
        assert_eq!(clock.observe(now), Err(HostError::InvalidRequest));
        assert_eq!(clock.timestamp(), Ok(17));
    }
    assert_eq!(clock.configure(1, 117), Err(HostError::InvalidRequest));
    assert_eq!(clock.timestamp(), Ok(17));
    clock.observe(118).unwrap();
    assert_eq!(clock.timestamp(), Ok(18));
}

#[test]
fn explicit_advance_rejects_host_time_overflow_without_wrap() {
    let mut clock = CpuClock::default();
    clock.configure(1_000_000_000, u64::MAX - 1).unwrap();
    clock.advance(1).unwrap();
    assert_eq!(clock.timestamp(), Ok(1));
    assert_eq!(clock.advance(1), Err(HostError::InvalidRequest));
    assert_eq!(clock.timestamp(), Ok(1));
    clock.advance(0).unwrap();
    assert_eq!(clock.timestamp(), Ok(1));
}

#[test]
fn overflowing_ticks_reject_the_observation_without_committing_it() {
    let mut clock = CpuClock::default();
    clock.configure(u64::MAX, 0).unwrap();
    clock.observe(1_000_000_000).unwrap();
    assert_eq!(clock.timestamp(), Ok(u64::MAX));
    assert_eq!(clock.observe(1_000_000_001), Err(HostError::InvalidRequest));
    assert_eq!(clock.timestamp(), Ok(u64::MAX));
    clock.observe(1_000_000_000).unwrap();
}

#[test]
fn close_discards_the_clock_and_a_new_instance_requires_configuration() {
    let mut engine = EngineInstance::new(1, 7).unwrap();
    engine.configure_cpu_clock(1_000_000_000, 100).unwrap();
    engine.observe_cpu_clock(117).unwrap();
    assert_eq!(engine.cpu_clock.timestamp(), Ok(17));
    engine.close();
    assert_eq!(engine.cpu_clock.timestamp(), Err(HostError::Infrastructure));
    assert_eq!(engine.configure_cpu_clock(1, 0), Err(HostError::Closed));
    assert_eq!(engine.observe_cpu_clock(118), Err(HostError::Closed));
    assert_eq!(engine.advance_cpu_clock(1), Err(HostError::Closed));
    let mut next = EngineInstance::new(1, 8).unwrap();
    assert_eq!(next.cpu_clock.timestamp(), Err(HostError::Infrastructure));
    next.configure_cpu_clock(1, 0).unwrap();
    assert_eq!(next.cpu_clock.timestamp(), Ok(0));
}
