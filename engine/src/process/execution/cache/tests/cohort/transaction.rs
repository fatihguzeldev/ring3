use super::*;
use crate::cpu::{
    dbt::{CompileLimits, RegistryLimits, ResidentRegistry},
    x86::decode::DecodeAuthority,
};
use std::sync::atomic::Ordering;

pub(in crate::process::execution::cache::tests) fn chain(
    configured_bytes: Option<usize>,
) -> (EngineInstance, Vec<CacheRequest32>) {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    for index in 0..8 {
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(
                GuestAddress(0x1000 + index * 16),
                &[0x90, 0xeb, if index == 7 { 0xfd } else { 0x0d }],
            )
            .unwrap();
    }
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1080), &[0x90, 0xc3])
        .unwrap();
    let cs = engine.code_fetch_context().unwrap();
    let unit_bytes = configured_bytes.unwrap_or_else(|| {
        let mut probe =
            ResidentRegistry::new(engine.memory().unwrap(), RegistryLimits::default()).unwrap();
        let id = probe
            .compile_cached_bound(
                engine.memory().unwrap(),
                GuestAddress(0x1000),
                None,
                CompileLimits::default(),
                KEY,
                cs,
                DecodeAuthority::default(),
                CompilationTier32::Optimized,
                &|pc| pc.0 == 0x1010,
            )
            .unwrap();
        probe
            .get(engine.memory().unwrap(), id)
            .unwrap()
            .retained_wasm_byte_len()
            + 64
    });
    engine
        .configure_execution_with_limits(
            ExecutionConfig32 {
                units: 8,
                wasm_bytes: 1048576,
                promotion_hits: 8,
            },
            CompileLimits {
                wasm_bytes: unit_bytes,
                ..CompileLimits::default()
            },
        )
        .unwrap();
    let mut owners = Vec::new();
    for index in 0..7 {
        owners.push(install(&mut engine, 0x1000 + index * 16));
    }
    let id = engine
        .resident
        .as_mut()
        .unwrap()
        .compile_cached_bound(
            engine.memory.as_ref().unwrap(),
            GuestAddress(0x1070),
            None,
            CompileLimits {
                wasm_bytes: unit_bytes,
                ..CompileLimits::default()
            },
            KEY,
            cs,
            DecodeAuthority::default(),
            CompilationTier32::Optimized,
            &|_| false,
        )
        .unwrap()
        .get();
    engine.acknowledge_resident_slot(id, 7).unwrap();
    owners.push(CacheRequest32 {
        serial: 0,
        kind: CacheRequestKind32::Install,
        pc: 0x1070,
        unit_id: id,
        victim_id: 0,
        slot: 7,
        victim_slot: u32::MAX,
        tier: CompilationTier32::Optimized,
    });
    for _ in 0..8 {
        engine.lookup_installed_resident(KEY, 0x1000).unwrap();
    }
    (engine, owners)
}

fn retained(engine: &EngineInstance, owners: &[CacheRequest32]) -> Vec<(usize, Vec<u8>, u64, u64)> {
    owners
        .iter()
        .map(|owner| {
            let bytes = engine.resident_bytes(owner.unit_id).unwrap();
            let row = engine.execution_cache.as_ref().unwrap().entries[owner.slot as usize]
                .as_ref()
                .unwrap();
            (
                bytes.as_ptr() as usize,
                bytes.to_vec(),
                row.hits.load(Ordering::Relaxed),
                row.last.load(Ordering::Relaxed),
            )
        })
        .collect()
}

fn assert_retained(
    engine: &EngineInstance,
    owners: &[CacheRequest32],
    before: &[(usize, Vec<u8>, u64, u64)],
) {
    assert_eq!(retained(engine, owners), before);
    for owner in owners {
        let row = engine.execution_cache.as_ref().unwrap().entries[owner.slot as usize]
            .as_ref()
            .unwrap();
        assert_eq!((row.id, row.tier), (owner.unit_id, owner.tier));
        assert_eq!(
            engine
                .resident_installation_by_id(owner.unit_id)
                .unwrap()
                .slot,
            owner.slot
        );
        assert_eq!(
            engine.lookup_resident(owner.pc).unwrap().get(),
            owner.unit_id
        );
    }
}

#[test]
fn byte_shortening_declines_before_retiring_a_mature_optimized_neighbor() {
    let (mut engine, owners) = chain(None);
    let before = retained(&engine, &owners);
    let usage = engine.cache_usage().unwrap();
    assert!(engine.prepare_cache_promotion(KEY).unwrap().is_none());
    assert_eq!(engine.cache_usage().unwrap(), usage);
    assert!(engine.pending_cache_request().unwrap().is_none());
    assert_retained(&engine, &owners, &before);
}

#[test]
fn mixed_tier_replacement_fits_after_victims_and_republishes_a_removed_index_page() {
    let (mut engine, owners) = chain(Some(65536));
    let before = retained(&engine, &owners);
    let usage = engine.cache_usage().unwrap();
    assert_eq!(usage.units, 8);
    let mut request = engine.prepare_cache_promotion(KEY).unwrap().unwrap();
    assert_eq!(engine.cache_usage().unwrap().units, 8);
    assert_retained(&engine, &owners, &before);
    for owner in &owners {
        assert_eq!(request.victim_id, owner.unit_id);
        assert_eq!(request.unit_id, 0);
        acknowledge(&mut engine, request);
        if owner.unit_id != owners.last().unwrap().unit_id {
            request = engine.prepare_cache_entry(KEY, 0x1000).unwrap();
        }
    }
    assert_eq!(engine.cache_usage().unwrap().units, 0);
    request = engine.prepare_cache_entry(KEY, 0x1000).unwrap();
    assert_eq!(
        (request.kind, request.tier),
        (CacheRequestKind32::Install, CompilationTier32::Optimized)
    );
    engine
        .install_cached_entry(KEY, request.serial, request.unit_id, request.slot)
        .unwrap();
    assert_eq!(engine.cache_usage().unwrap().units, 1);
    for owner in owners {
        for pc in [owner.pc, owner.pc + 1] {
            assert_eq!(
                engine.lookup_installed_resident(KEY, pc).unwrap().unit_id,
                request.unit_id
            );
        }
    }
}

#[test]
fn cancellation_before_first_ack_drops_only_the_unpublished_replacement() {
    let (mut engine, owners) = chain(Some(65536));
    let before = retained(&engine, &owners);
    let request = engine.prepare_cache_promotion(KEY).unwrap().unwrap();
    engine.cancel_cache_request(KEY, request.serial).unwrap();
    assert!(
        engine
            .execution_cache
            .as_ref()
            .unwrap()
            .replacement
            .is_none()
    );
    assert_eq!(engine.cache_usage().unwrap().evictions, 0);
    assert_retained(&engine, &owners, &before);
}

#[test]
fn interleaved_publication_is_rejected_before_first_ack_without_clearing_old_owners() {
    let (mut engine, owners, unrelated) = fixture();
    engine
        .discard_installed_resident(KEY, unrelated.unit_id, unrelated.slot)
        .unwrap();
    let before = retained(&engine, &owners);
    let request = engine.prepare_cache_promotion(KEY).unwrap().unwrap();
    let cs = engine.code_fetch_context().unwrap();
    let id = engine
        .resident
        .as_mut()
        .unwrap()
        .compile_cached_bound(
            engine.memory.as_ref().unwrap(),
            GuestAddress(0x1080),
            None,
            CompileLimits::default(),
            KEY,
            cs,
            DecodeAuthority::default(),
            CompilationTier32::Baseline,
            &|_| false,
        )
        .unwrap()
        .get();
    let bytes = engine.resident_bytes(id).unwrap().to_vec();
    assert_eq!(
        engine.acknowledge_cache_eviction(
            KEY,
            request.serial,
            request.victim_id,
            request.victim_slot
        ),
        Err(HostError::Call(CallError::StateChanged))
    );
    assert_retained(&engine, &owners, &before);
    assert_eq!(engine.resident_bytes(id).unwrap(), bytes);
    engine.cancel_cache_request(KEY, request.serial).unwrap();
}
