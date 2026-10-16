use super::admission::{KEY, fixture, install};
use crate::{
    cpu::dbt::CompilationTier32,
    memory::GuestAddress,
    process::{CallError, HostError, execution::cache::CacheRequestKind32},
};

#[test]
fn hot_baseline_promotes_only_after_quiescent_eviction_and_new_installation() {
    let mut engine = fixture();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1000), &[0x90, 0x90, 0x90, 0xc3])
        .unwrap();
    let baseline = install(&mut engine, 0x1000);
    assert!(engine.prepare_cache_promotion(KEY).unwrap().is_none());
    for _ in 0..8 {
        engine.lookup_installed_resident(KEY, 0x1000).unwrap();
    }
    let bytes = engine.resident_bytes(baseline.unit_id).unwrap().to_vec();
    let promotion = engine.prepare_cache_promotion(KEY).unwrap().unwrap();
    assert_eq!(
        (promotion.kind, promotion.tier, promotion.victim_id),
        (
            CacheRequestKind32::Evict,
            CompilationTier32::Optimized,
            baseline.unit_id
        )
    );
    assert_eq!(engine.cache_usage().unwrap().promotions, 0);
    assert_eq!(engine.resident_bytes(baseline.unit_id).unwrap(), bytes);
    engine
        .acknowledge_cache_eviction(
            KEY,
            promotion.serial,
            promotion.victim_id,
            promotion.victim_slot,
        )
        .unwrap();
    let optimized = engine.prepare_cache_entry(KEY, promotion.pc).unwrap();
    assert_eq!(optimized.tier, CompilationTier32::Optimized);
    assert_ne!(optimized.unit_id, baseline.unit_id);
    assert_eq!(engine.cache_usage().unwrap().promotions, 0);
    assert_ne!(engine.resident_bytes(optimized.unit_id).unwrap(), bytes);
    engine
        .install_cached_entry(KEY, optimized.serial, optimized.unit_id, optimized.slot)
        .unwrap();
    assert_eq!(engine.cache_usage().unwrap().promotions, 1);
    assert_eq!(
        engine.prepare_cache_entry(KEY, 0x1000).unwrap().tier,
        CompilationTier32::Optimized
    );
    for _ in 0..8 {
        engine.lookup_installed_resident(KEY, 0x1000).unwrap();
    }
    assert!(engine.prepare_cache_promotion(KEY).unwrap().is_none());
}
#[test]
fn promotion_source_change_invalidates_receipt_and_falls_back_to_fresh_baseline() {
    let mut engine = fixture();
    let first = install(&mut engine, 0x1000);
    for _ in 0..8 {
        engine.lookup_installed_resident(KEY, 0x1000).unwrap();
    }
    let promotion = engine.prepare_cache_promotion(KEY).unwrap().unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1000), &[0x91])
        .unwrap();
    assert_eq!(
        engine.acknowledge_cache_eviction(KEY, promotion.serial, first.unit_id, first.slot),
        Err(HostError::Call(CallError::StateChanged))
    );
    engine.cancel_cache_request(KEY, promotion.serial).unwrap();
    assert!(engine.prepare_cache_promotion(KEY).unwrap().is_none());
    let eviction = engine.prepare_cache_entry(KEY, 0x1000).unwrap();
    if eviction.kind == CacheRequestKind32::Evict {
        engine
            .acknowledge_cache_eviction(
                KEY,
                eviction.serial,
                eviction.victim_id,
                eviction.victim_slot,
            )
            .unwrap();
    }
    let candidate = if eviction.kind == CacheRequestKind32::Install {
        eviction
    } else {
        engine.prepare_cache_entry(KEY, 0x1000).unwrap()
    };
    assert_eq!(candidate.tier, CompilationTier32::Baseline);
}
#[test]
fn source_change_after_eviction_cannot_inherit_optimized_authority() {
    let mut engine = fixture();
    let first = install(&mut engine, 0x1000);
    for _ in 0..8 {
        engine.lookup_installed_resident(KEY, 0x1000).unwrap();
    }
    let promotion = engine.prepare_cache_promotion(KEY).unwrap().unwrap();
    engine
        .acknowledge_cache_eviction(KEY, promotion.serial, first.unit_id, first.slot)
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1000), &[0x91])
        .unwrap();
    assert_eq!(
        engine.prepare_cache_entry(KEY, 0x1000).unwrap().tier,
        CompilationTier32::Baseline
    );
}

#[test]
fn hot_single_instruction_unit_never_claims_an_unemitted_index_optimization() {
    let mut engine = fixture();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1000), &[0xc3])
        .unwrap();
    let baseline = install(&mut engine, 0x1000);
    for _ in 0..8 {
        engine.lookup_installed_resident(KEY, 0x1000).unwrap();
    }
    assert!(engine.prepare_cache_promotion(KEY).unwrap().is_none());
    assert_eq!(
        engine
            .resident
            .as_ref()
            .unwrap()
            .get_raw(engine.memory.as_ref().unwrap(), baseline.unit_id)
            .unwrap()
            .tier(),
        CompilationTier32::Baseline
    );
    assert_eq!(engine.cache_usage().unwrap().promotions, 0);
}
