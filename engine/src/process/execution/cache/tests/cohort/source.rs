use super::*;

#[test]
fn fresh_private_rw_preserves_old_authority_before_and_between_cohort_acknowledgements() {
    for phase in 0..3 {
        let (mut engine, owners, unrelated) = fixture();
        engine.unmap(0x8000, 1).unwrap();
        let first = engine.prepare_cache_promotion(KEY).unwrap().unwrap();
        let mut request = first;
        if phase != 0 {
            acknowledge(&mut engine, first);
            if phase == 1 {
                request = engine.prepare_cache_entry(KEY, 0x1000).unwrap();
            }
        }
        let remaining = &owners[usize::from(phase != 0)..];
        let retained = remaining
            .iter()
            .chain(std::iter::once(&unrelated))
            .map(|owner| {
                (
                    owner.unit_id,
                    owner.slot,
                    engine.resident_bytes(owner.unit_id).unwrap().to_vec(),
                )
            })
            .collect::<Vec<_>>();
        let usage = engine.cache_usage().unwrap();
        let source = engine.memory().unwrap().code_source_version();
        let transaction = engine.memory().unwrap().transaction_version();
        engine.map(0x9000, 1, 3).unwrap();
        assert_eq!(engine.memory().unwrap().code_source_version(), source);
        assert_eq!(
            engine.memory().unwrap().code_validation_version(),
            transaction + 1
        );
        if phase == 2 {
            assert_eq!(
                engine.prepare_cache_entry(KEY, 0x1000),
                Err(HostError::Call(CallError::StateChanged))
            );
            assert!(engine.execution_cache.as_ref().unwrap().cohort.is_none());
            assert!(
                engine
                    .execution_cache
                    .as_ref()
                    .unwrap()
                    .replacement
                    .is_none()
            );
        } else {
            assert_eq!(
                engine.acknowledge_cache_eviction(
                    KEY,
                    request.serial,
                    request.victim_id,
                    request.victim_slot
                ),
                Err(HostError::Call(CallError::StateChanged))
            );
            assert_eq!(engine.pending_cache_request().unwrap(), Some(request));
            assert_eq!(engine.cache_usage().unwrap(), usage);
            engine.cancel_cache_request(KEY, request.serial).unwrap();
        }
        let mut cancelled_usage = usage;
        cancelled_usage.pending_serial = 0;
        assert_eq!(engine.cache_usage().unwrap(), cancelled_usage);
        assert!(engine.pending_cache_request().unwrap().is_none());
        assert!(engine.execution_cache.as_ref().unwrap().cohort.is_none());
        assert!(
            engine
                .execution_cache
                .as_ref()
                .unwrap()
                .replacement
                .is_none()
        );
        for (id, slot, bytes) in retained {
            assert_eq!(engine.resident_installation_by_id(id).unwrap().slot, slot);
            assert_eq!(engine.resident_bytes(id).unwrap(), bytes);
        }
        assert_eq!(
            engine.cache_usage().unwrap().evictions,
            u64::from(phase != 0)
        );
    }
}
