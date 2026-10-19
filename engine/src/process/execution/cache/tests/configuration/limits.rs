use crate::{
    abi::arena::TRANSFER_OFFSET,
    cpu::dbt::{CompilationTier32, CompileError, CompileLimits, RegistryError},
    memory::GuestAddress,
    process::{CallError, EngineInstance, ExecutionConfig32, HostError},
};

const KEY: u64 = 97;

fn fixture() -> EngineInstance {
    let mut engine = EngineInstance::new(1, KEY).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    let mut code = vec![0x90; 96];
    code.extend([0xff, 0xe2]);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1000), &code)
        .unwrap();
    engine
}

fn config() -> ExecutionConfig32 {
    ExecutionConfig32 {
        units: 8,
        wasm_bytes: 1_048_576,
        promotion_hits: 2,
    }
}

fn larger_limits() -> CompileLimits {
    CompileLimits {
        instructions: 128,
        wasm_bytes: 131_072,
        ..CompileLimits::default()
    }
}

#[test]
fn explicit_default_unit_limits_preserve_cache_request_shape_and_instruction_bound() {
    let mut results = Vec::new();
    for explicit in [false, true] {
        let mut engine = fixture();
        if explicit {
            engine
                .configure_execution_with_limits(config(), CompileLimits::default())
                .unwrap();
        } else {
            engine.configure_execution(config()).unwrap();
        }
        assert_eq!(engine.execution_config().unwrap(), Some(config()));
        assert_eq!(
            engine.execution_cache.as_ref().unwrap().compile_limits,
            CompileLimits::default()
        );
        let request = engine.prepare_cache_entry(KEY, 0x1000).unwrap();
        assert_ne!(request.unit_id, 0);
        let region = engine
            .resident
            .as_ref()
            .unwrap()
            .get_raw(engine.memory.as_ref().unwrap(), request.unit_id)
            .unwrap();
        assert_eq!(region.metadata().instructions, 64);
        assert!(region.contains_instruction(0x103f));
        assert!(!region.contains_instruction(0x1040));
        results.push((
            request.serial,
            request.kind,
            request.pc,
            request.victim_id,
            request.slot,
            request.victim_slot,
            request.tier,
        ));
    }
    assert_eq!(results[0], results[1]);
}

#[test]
fn larger_cache_units_retain_more_than_sixty_four_instructions_and_promote_them() {
    let mut engine = fixture();
    engine
        .configure_execution_with_limits(config(), larger_limits())
        .unwrap();
    let first = engine.prepare_cache_entry(KEY, 0x1000).unwrap();
    engine
        .install_cached_entry(KEY, first.serial, first.unit_id, first.slot)
        .unwrap();
    for _ in 0..2 {
        engine.lookup_installed_resident(KEY, 0x1000).unwrap();
    }
    let promotion = engine.prepare_cache_promotion(KEY).unwrap().unwrap();
    assert_eq!(promotion.victim_id, first.unit_id);
    engine
        .acknowledge_cache_eviction(
            KEY,
            promotion.serial,
            promotion.victim_id,
            promotion.victim_slot,
        )
        .unwrap();
    let promoted = engine.prepare_cache_entry(KEY, 0x1000).unwrap();
    assert_eq!(promoted.tier, CompilationTier32::Optimized);
    engine
        .install_cached_entry(KEY, promoted.serial, promoted.unit_id, promoted.slot)
        .unwrap();
    let region = engine
        .resident
        .as_ref()
        .unwrap()
        .get_raw(engine.memory.as_ref().unwrap(), promoted.unit_id)
        .unwrap();
    assert_eq!(region.metadata().instructions, 97);
    assert!(region.contains_instruction(0x1040));
    assert!(region.contains_instruction(0x1060));
    assert!(
        region
            .wasm_bytes(engine.memory.as_ref().unwrap())
            .unwrap()
            .len()
            <= 131_072
    );
    assert_eq!(
        engine
            .lookup_installed_resident(KEY, 0x1060)
            .unwrap()
            .unit_id,
        promoted.unit_id
    );
}

#[test]
fn larger_cache_unit_policy_does_not_widen_manual_resident_compilation() {
    let mut engine = fixture();
    engine
        .configure_execution_with_limits(config(), larger_limits())
        .unwrap();
    let transfer = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    transfer[..4].copy_from_slice(&0x1000u32.to_le_bytes());
    transfer[4..].copy_from_slice(&98u32.to_le_bytes());
    assert_eq!(
        engine.compile_resident(1),
        Err(HostError::Resident(RegistryError::Compile(
            CompileError::InstructionLimit
        )))
    );
    assert_eq!(engine.cache_usage().unwrap().units, 0);
}

#[test]
fn invalid_unit_limits_leave_pristine_configuration_and_arena_unchanged() {
    let mut engine = fixture();
    let dispatcher = engine.dispatcher_bytes(KEY).unwrap().to_vec();
    let arena = engine.arena().to_vec();
    for limits in [
        CompileLimits {
            blocks: 0,
            ..larger_limits()
        },
        CompileLimits {
            blocks: 7,
            ..larger_limits()
        },
        CompileLimits {
            blocks: 9,
            ..larger_limits()
        },
        CompileLimits {
            instructions: 0,
            ..larger_limits()
        },
        CompileLimits {
            instructions: 129,
            ..larger_limits()
        },
        CompileLimits {
            instructions: usize::MAX,
            ..larger_limits()
        },
        CompileLimits {
            wasm_bytes: 0,
            ..larger_limits()
        },
        CompileLimits {
            wasm_bytes: 131_073,
            ..larger_limits()
        },
        CompileLimits {
            wasm_bytes: usize::MAX,
            ..larger_limits()
        },
    ] {
        assert_eq!(
            engine.configure_execution_with_limits(config(), limits),
            Err(HostError::Resident(RegistryError::InvalidLimits))
        );
        assert_eq!(engine.execution_config().unwrap(), None);
        assert!(engine.resident.is_none());
        assert_eq!(engine.resident_installations.len(), 8);
        assert!(engine.resident_installations_by_id.is_empty());
        assert_eq!(engine.dispatcher_bytes(KEY).unwrap(), dispatcher);
        assert_eq!(engine.arena(), arena);
    }
    assert_eq!(
        engine.configure_execution_with_limits(
            ExecutionConfig32 {
                wasm_bytes: 65_536,
                ..config()
            },
            larger_limits(),
        ),
        Err(HostError::Resident(RegistryError::InvalidLimits))
    );
    assert_eq!(engine.execution_config().unwrap(), None);
    assert_eq!(engine.dispatcher_bytes(KEY).unwrap(), dispatcher);
    assert_eq!(engine.arena(), arena);
    engine
        .configure_execution_with_limits(config(), larger_limits())
        .unwrap();
    assert_eq!(engine.execution_config().unwrap(), Some(config()));
}

#[test]
fn configured_unit_policy_is_immutable_and_busy_precedes_new_limit_validation() {
    let mut engine = fixture();
    engine
        .configure_execution_with_limits(config(), larger_limits())
        .unwrap();
    let dispatcher = engine.dispatcher_bytes(KEY).unwrap().to_vec();
    let usage = engine.cache_usage().unwrap();
    assert_eq!(
        engine.configure_execution(config()),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.configure_execution_with_limits(
            config(),
            CompileLimits {
                instructions: 0,
                ..larger_limits()
            },
        ),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.execution_cache.as_ref().unwrap().compile_limits,
        larger_limits()
    );
    assert_eq!(engine.dispatcher_bytes(KEY).unwrap(), dispatcher);
    assert_eq!(engine.cache_usage().unwrap(), usage);
    engine.close();
    assert_eq!(
        engine.configure_execution_with_limits(config(), larger_limits()),
        Err(HostError::Closed)
    );
}

#[test]
fn cache_admission_reserves_the_selected_unit_byte_limit_before_publication() {
    use crate::process::CacheRequestKind32;
    for explicit in [false, true] {
        let mut engine = fixture();
        for pc in [0x1000, 0x1100] {
            engine
                .memory
                .as_mut()
                .unwrap()
                .write(GuestAddress(pc), &[0x90, 0xff, 0xe2])
                .unwrap();
        }
        let config = ExecutionConfig32 {
            units: 2,
            wasm_bytes: 131_072,
            ..config()
        };
        if explicit {
            engine
                .configure_execution_with_limits(config, larger_limits())
                .unwrap();
        } else {
            engine.configure_execution(config).unwrap();
        }
        let first = engine.prepare_cache_entry(KEY, 0x1000).unwrap();
        engine
            .install_cached_entry(KEY, first.serial, first.unit_id, first.slot)
            .unwrap();
        let second = engine.prepare_cache_entry(KEY, 0x1100).unwrap();
        if explicit {
            assert_eq!(second.kind, CacheRequestKind32::Evict);
            assert_eq!(second.victim_id, first.unit_id);
            assert_eq!(engine.cache_usage().unwrap().units, 1);
            engine
                .acknowledge_cache_eviction(
                    KEY,
                    second.serial,
                    second.victim_id,
                    second.victim_slot,
                )
                .unwrap();
            let retried = engine.prepare_cache_entry(KEY, 0x1100).unwrap();
            assert_eq!(retried.kind, CacheRequestKind32::Install);
        } else {
            assert_eq!(second.kind, CacheRequestKind32::Install);
            assert_eq!(engine.cache_usage().unwrap().units, 2);
        }
    }
}

#[test]
fn cache_promotion_plans_whole_owners_beyond_the_default_instruction_limit() {
    use crate::process::CacheRequestKind32;
    let mut engine = fixture();
    let pcs = [0x1000u32, 0x1040, 0x1080];
    for (index, pc) in pcs.into_iter().enumerate() {
        let mut code = vec![0x90; 32];
        if let Some(&next) = pcs.get(index + 1) {
            code.push(0xe9);
            code.extend(next.wrapping_sub(pc + 37).to_le_bytes());
        } else {
            code.extend([0xff, 0xe2]);
        }
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(pc), &code)
            .unwrap();
    }
    engine
        .configure_execution_with_limits(config(), larger_limits())
        .unwrap();
    let mut owners = Vec::new();
    for (index, pc) in pcs.into_iter().enumerate() {
        let transfer = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
        transfer[..4].copy_from_slice(&pc.to_le_bytes());
        transfer[4..].copy_from_slice(&(if index < 2 { 37u32 } else { 34 }).to_le_bytes());
        let id = engine.compile_resident(1).unwrap().get();
        let request = engine.prepare_cache_entry(KEY, pc).unwrap();
        assert_eq!(request.unit_id, id);
        engine
            .install_cached_entry(KEY, request.serial, id, request.slot)
            .unwrap();
        owners.push(id);
    }
    for _ in 0..2 {
        engine.lookup_installed_resident(KEY, 0x1000).unwrap();
    }
    let mut request = engine.prepare_cache_promotion(KEY).unwrap().unwrap();
    for owner in owners {
        assert_eq!(request.kind, CacheRequestKind32::Evict);
        assert_eq!(request.victim_id, owner);
        engine
            .acknowledge_cache_eviction(KEY, request.serial, request.victim_id, request.victim_slot)
            .unwrap();
        request = engine.prepare_cache_entry(KEY, 0x1000).unwrap();
    }
    assert_eq!(request.kind, CacheRequestKind32::Install);
    assert_eq!(request.tier, CompilationTier32::Optimized);
    engine
        .install_cached_entry(KEY, request.serial, request.unit_id, request.slot)
        .unwrap();
    let region = engine
        .resident
        .as_ref()
        .unwrap()
        .get_raw(engine.memory.as_ref().unwrap(), request.unit_id)
        .unwrap();
    assert_eq!(
        (region.metadata().blocks, region.metadata().instructions),
        (3, 99)
    );
    assert_eq!(engine.cache_usage().unwrap().evictions, 3);
    for pc in pcs {
        assert_eq!(
            engine.lookup_installed_resident(KEY, pc).unwrap().unit_id,
            request.unit_id
        );
    }
}
