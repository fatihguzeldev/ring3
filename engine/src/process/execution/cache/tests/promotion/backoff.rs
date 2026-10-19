use super::super::cohort::transaction::chain;
use super::*;
use std::sync::atomic::Ordering;

fn attempts(engine: &crate::process::EngineInstance) -> u64 {
    engine
        .execution_cache
        .as_ref()
        .unwrap()
        .promotion_attempts
        .load(Ordering::Relaxed)
}

#[test]
fn real_byte_declines_skip_complete_rounds_then_reconsider_at_the_finite_cap() {
    let (mut engine, _) = chain(None);
    assert!(engine.prepare_cache_promotion(KEY).unwrap().is_none());
    let mut expected = 1;
    assert_eq!(attempts(&engine), expected);
    for skipped in [1, 2, 4, 8, 16, 32, 64, 64] {
        for _ in 0..skipped {
            assert!(engine.prepare_cache_promotion(KEY).unwrap().is_none());
            assert_eq!(attempts(&engine), expected);
        }
        assert!(engine.prepare_cache_promotion(KEY).unwrap().is_none());
        expected += 1;
        assert_eq!(attempts(&engine), expected);
    }
    assert_eq!(engine.cache_usage().unwrap().evictions, 0);
}

#[test]
fn a_delayed_hottest_root_does_not_block_another_eligible_current_owner() {
    let (mut engine, _) = chain(None);
    for _ in 0..8 {
        engine.lookup_installed_resident(KEY, 0x1000).unwrap();
    }
    assert!(engine.prepare_cache_promotion(KEY).unwrap().is_none());
    for _ in 0..8 {
        engine.lookup_installed_resident(KEY, 0x1010).unwrap();
    }
    assert!(engine.prepare_cache_promotion(KEY).unwrap().is_none());
    assert_eq!(attempts(&engine), 2);
    assert!(
        !engine.execution_cache.as_ref().unwrap().entries[1]
            .as_ref()
            .unwrap()
            .promotion_retry
            .eligible(2)
    );
    assert_eq!(engine.cache_usage().unwrap().evictions, 0);
}

#[test]
fn a_new_row_identity_and_round_rollover_cannot_inherit_permanent_suppression() {
    let (mut engine, owners) = chain(None);
    assert!(engine.prepare_cache_promotion(KEY).unwrap().is_none());
    engine
        .discard_installed_resident(KEY, owners[0].unit_id, owners[0].slot)
        .unwrap();
    let new = install(&mut engine, 0x1000);
    assert_ne!(new.unit_id, owners[0].unit_id);
    for _ in 0..8 {
        engine.lookup_installed_resident(KEY, 0x1000).unwrap();
    }
    assert!(engine.prepare_cache_promotion(KEY).unwrap().is_none());
    assert_eq!(attempts(&engine), 2);
    engine.execution_cache.as_mut().unwrap().promotion_round = u64::MAX;
    assert!(engine.prepare_cache_promotion(KEY).unwrap().is_none());
    assert_eq!(engine.execution_cache.as_ref().unwrap().promotion_round, 1);
    assert_eq!(attempts(&engine), 3);
    assert!(engine.prepare_cache_promotion(KEY).unwrap().is_none());
    assert_eq!(attempts(&engine), 3);
}

#[test]
fn cancelling_a_valid_staged_candidate_does_not_delay_its_unchanged_root() {
    let (mut engine, _) = chain(Some(65536));
    let first = engine.prepare_cache_promotion(KEY).unwrap().unwrap();
    engine.cancel_cache_request(KEY, first.serial).unwrap();
    let second = engine.prepare_cache_promotion(KEY).unwrap().unwrap();
    assert_ne!(second.serial, first.serial);
    assert_eq!(attempts(&engine), 2);
    assert_eq!(engine.cache_usage().unwrap().evictions, 0);
}

#[cfg(feature = "observability")]
#[test]
fn basic_instance_observation_records_declines_but_not_skipped_retry_rounds() {
    use ring3_observability::{CategorySelection, Config, Level, Observer, categories};
    let (mut engine, _) = chain(None);
    let observer = Observer::new(Config {
        level: Level::Basic,
        categories: CategorySelection::Only(vec![categories::ENGINE_CPU_CACHE]),
        ..Config::default()
    })
    .unwrap();
    engine.set_observer(observer.clone()).unwrap();
    let before = observer.stats().emitted_events;
    assert!(engine.prepare_cache_promotion(KEY).unwrap().is_none());
    assert_eq!(observer.stats().emitted_events, before + 1);
    assert!(engine.prepare_cache_promotion(KEY).unwrap().is_none());
    assert_eq!(observer.stats().emitted_events, before + 1);
    let bytes = observer.drain();
    for field in [
        b"promotion.declined".as_slice(),
        b"unit_id",
        b"pc",
        b"retry_rounds",
        b"next_promotion_round",
    ] {
        assert!(bytes.windows(field.len()).any(|window| window == field));
    }
    assert_eq!(observer.stats().dropped_events, 0);
}
