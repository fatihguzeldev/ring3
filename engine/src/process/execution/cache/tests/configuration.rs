use crate::{
    abi::arena::TRANSFER_OFFSET,
    cpu::dbt::RegistryError,
    memory::GuestAddress,
    process::{EngineInstance, HostError, execution::cache::ExecutionConfig32},
};

#[test]
fn configured_registry_installs_and_resolves_a_ninth_unit_without_changing_legacy_limits() {
    for configured in [false, true] {
        let mut engine = EngineInstance::new(2, 97).unwrap();
        engine.map(0x1000, 1, 7).unwrap();
        if configured {
            engine
                .configure_execution(ExecutionConfig32 {
                    units: 16,
                    ..ExecutionConfig32::default()
                })
                .unwrap();
            assert!(engine.resident_installations_by_id.is_empty());
            assert!(engine.resident_installations_by_id.capacity() >= 16);
        }
        for index in 0..9 {
            let entry = 0x1000 + index * 16;
            engine
                .memory
                .as_mut()
                .unwrap()
                .write(GuestAddress(entry), &[0x90, 0xeb, 0xfd])
                .unwrap();
            let transfer = &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
            transfer[..4].copy_from_slice(&entry.to_le_bytes());
            transfer[4..].copy_from_slice(&3u32.to_le_bytes());
            let compiled = engine.compile_resident(1);
            if !configured && index == 8 {
                assert_eq!(
                    compiled,
                    Err(HostError::Resident(RegistryError::UnitCapacity))
                );
                break;
            }
            let id = compiled.unwrap().get();
            engine
                .acknowledge_resident_installation(97, id, index)
                .unwrap();
            let installed = engine.lookup_installed_resident(97, entry).unwrap();
            assert_eq!((installed.unit_id, installed.slot), (id, index));
        }
    }
}

#[test]
fn rejected_configuration_preserves_dispatcher_and_legacy_storage() {
    let mut engine = EngineInstance::new(1, 97).unwrap();
    let bytes = engine.dispatcher_bytes(97).unwrap().to_vec();
    for config in [
        ExecutionConfig32 {
            units: 0,
            ..ExecutionConfig32::default()
        },
        ExecutionConfig32 {
            units: crate::cpu::dbt::RegistryLimits::MAX_UNITS as u32 + 1,
            ..ExecutionConfig32::default()
        },
        ExecutionConfig32 {
            wasm_bytes: 1,
            ..ExecutionConfig32::default()
        },
        ExecutionConfig32 {
            promotion_hits: 0,
            ..ExecutionConfig32::default()
        },
    ] {
        assert_eq!(
            engine.configure_execution(config),
            Err(HostError::Resident(RegistryError::InvalidLimits))
        );
        assert_eq!(engine.dispatcher_bytes(97).unwrap(), bytes);
        assert_eq!(engine.resident_installations.len(), 8);
        assert!(engine.resident_installations_by_id.is_empty());
        assert!(engine.resident.is_none());
        assert_eq!(engine.execution_config().unwrap(), None);
    }
}

#[test]
fn maximum_bounded_capacity_configures_without_changing_default_or_legacy_capacity() {
    let mut engine = EngineInstance::new(1, 97).unwrap();
    let config = ExecutionConfig32 {
        units: crate::cpu::dbt::RegistryLimits::MAX_UNITS as u32,
        ..ExecutionConfig32::default()
    };
    assert_eq!(ExecutionConfig32::default().units, 256);
    engine.configure_execution(config).unwrap();
    assert_eq!(engine.execution_config().unwrap(), Some(config));
    assert_eq!(engine.resident_installations.len(), 65_536);
    assert_eq!(engine.cache_usage().unwrap().installed, 0);
    assert_eq!(
        engine.execution_cache.as_ref().unwrap().free_slots.first(),
        Some(0)
    );
    assert!(engine.dispatcher_bytes(97).unwrap().len() < 1024);
}
