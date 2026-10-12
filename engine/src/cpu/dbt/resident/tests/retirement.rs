use super::*;
use crate::memory::{PageRange, Permissions};

fn fresh_memory(pages: u32) -> AddressSpace {
    let mut memory = AddressSpace::new(pages).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), pages).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    for page in 0..pages {
        memory
            .write(GuestAddress(0x1000 + page * 4096), &[0x90, 0xeb, 0])
            .unwrap();
    }
    memory
}

fn compile(registry: &mut ResidentRegistry, memory: &AddressSpace, pc: u32) -> UnitId {
    registry
        .compile(
            memory,
            &[BlockSpec {
                entry: GuestAddress(pc),
                byte_length: 3,
            }],
            CompileLimits::default(),
        )
        .unwrap()
}

fn module(registry: &ResidentRegistry, memory: &AddressSpace, id: UnitId) -> (usize, Vec<u8>) {
    let bytes = registry
        .get(memory, id)
        .unwrap()
        .wasm_bytes(memory)
        .unwrap();
    (bytes.as_ptr() as usize, bytes.to_vec())
}

#[test]
fn retirement_reclaims_exact_unit_and_byte_credit_without_moving_current_bytes() {
    let mut memory = fresh_memory(8);
    let mut registry = ResidentRegistry::new(&memory, RegistryLimits::default()).unwrap();
    let ids: Vec<_> = (0..8)
        .map(|page| compile(&mut registry, &memory, 0x1000 + page * 4096))
        .collect();
    let keepers: Vec<_> = ids[1..]
        .iter()
        .map(|&id| module(&registry, &memory, id))
        .collect();
    let removed_bytes = module(&registry, &memory, ids[0]).1.len();
    let before = registry.usage();
    memory.write(GuestAddress(0x1000), &[0x90]).unwrap();
    assert_eq!(
        registry.compile(
            &memory,
            &[BlockSpec {
                entry: GuestAddress(0x1000),
                byte_length: 3,
            }],
            CompileLimits::default(),
        ),
        Err(RegistryError::UnitCapacity)
    );
    registry.retire_stale(&memory, ids[0]).unwrap();
    assert_eq!(
        registry.usage(),
        RegistryUsage {
            units: 7,
            wasm_bytes: before.wasm_bytes - removed_bytes,
        }
    );
    assert_eq!(
        registry.get(&memory, ids[0]).unwrap_err(),
        RegistryError::InvalidUnit
    );
    for (&id, saved) in ids[1..].iter().zip(keepers) {
        assert_eq!(module(&registry, &memory, id), saved);
    }
    let fresh = compile(&mut registry, &memory, 0x1000);
    assert!(fresh.get() > ids[0].get() && !ids.contains(&fresh));
    assert_eq!(registry.usage().units, 8);
    assert_eq!(registry.lookup(&memory, GuestAddress(0x1000)), Ok(fresh));
}

#[test]
fn byte_capacity_is_reusable_only_after_explicit_stale_retirement() {
    let mut memory = fresh_memory(1);
    let specs = [BlockSpec {
        entry: GuestAddress(0x1000),
        byte_length: 3,
    }];
    let bytes = compile_region(&memory, &specs, CompileLimits::default())
        .unwrap()
        .wasm_bytes(&memory)
        .unwrap()
        .len();
    let mut registry = ResidentRegistry::new(
        &memory,
        RegistryLimits {
            units: 8,
            wasm_bytes: bytes * 2 - 1,
        },
    )
    .unwrap();
    let old = compile(&mut registry, &memory, 0x1000);
    memory.write(GuestAddress(0x1000), &[0x90]).unwrap();
    let before = registry.usage();
    assert_eq!(
        registry.compile(&memory, &specs, CompileLimits::default()),
        Err(RegistryError::ByteCapacity)
    );
    assert_eq!(registry.usage(), before);
    registry.retire_stale(&memory, old).unwrap();
    assert_eq!(
        registry.usage(),
        RegistryUsage {
            units: 0,
            wasm_bytes: 0
        }
    );
    let fresh = compile(&mut registry, &memory, 0x1000);
    assert!(fresh.get() > old.get());
    assert_eq!(
        registry.usage(),
        RegistryUsage {
            units: 1,
            wasm_bytes: bytes
        }
    );
}

#[test]
fn current_unknown_wrong_memory_and_repeated_retirement_preserve_registry() {
    let mut memory = fresh_memory(2);
    let wrong_memory = fresh_memory(1);
    let mut registry = ResidentRegistry::new(&memory, RegistryLimits::default()).unwrap();
    let target = compile(&mut registry, &memory, 0x1000);
    let keeper = compile(&mut registry, &memory, 0x2000);
    let before = registry.usage();
    let target_bytes = module(&registry, &memory, target);
    let keeper_bytes = module(&registry, &memory, keeper);
    for (id, expected) in [
        (target, RegistryError::CurrentUnit),
        (UnitId(0), RegistryError::InvalidUnit),
    ] {
        assert_eq!(registry.retire_stale(&memory, id), Err(expected));
        assert_eq!(registry.usage(), before);
        assert_eq!(module(&registry, &memory, target), target_bytes);
        assert_eq!(module(&registry, &memory, keeper), keeper_bytes);
    }
    assert_eq!(
        registry.retire_stale(&wrong_memory, UnitId(0)),
        Err(RegistryError::WrongAddressSpace)
    );
    assert_eq!(registry.usage(), before);
    memory.write(GuestAddress(0x1000), &[0x90]).unwrap();
    registry.retire_stale(&memory, target).unwrap();
    let retired = registry.usage();
    assert_eq!(
        registry.retire_stale(&memory, target),
        Err(RegistryError::InvalidUnit)
    );
    assert_eq!(registry.usage(), retired);
    assert_eq!(module(&registry, &memory, keeper), keeper_bytes);
}

#[test]
fn invalid_internal_byte_accounting_refuses_before_removal() {
    let mut memory = fresh_memory(2);
    let mut registry = ResidentRegistry::new(&memory, RegistryLimits::default()).unwrap();
    let target = compile(&mut registry, &memory, 0x1000);
    let keeper = compile(&mut registry, &memory, 0x2000);
    let bytes = module(&registry, &memory, target).1.len();
    let keeper_bytes = module(&registry, &memory, keeper);
    memory.write(GuestAddress(0x1000), &[0x90]).unwrap();
    // private fault injection exercises the accounting preflight before the drop.
    registry.wasm_bytes = bytes - 1;
    let before = registry.usage();
    assert_eq!(
        registry.retire_stale(&memory, target),
        Err(RegistryError::InvalidLimits)
    );
    assert_eq!(registry.usage(), before);
    assert_eq!(registry.check_stale_raw(&memory, target.get()), Ok(()));
    assert_eq!(module(&registry, &memory, keeper), keeper_bytes);
}

// these disposal proofs share the retirement fixture and inspect only retained owned bytes.
#[derive(Debug, PartialEq, Eq)]
struct Saved {
    usage: RegistryUsage,
    units: Vec<(u64, usize, Vec<u8>, bool)>,
}

fn saved(registry: &ResidentRegistry, memory: &AddressSpace) -> Saved {
    Saved {
        usage: registry.usage(),
        units: registry
            .entries
            .iter()
            .map(|unit| {
                let bytes = unit.region.retained_wasm_bytes_for_test();
                (
                    unit.id.get(),
                    bytes.as_ptr() as usize,
                    bytes.to_vec(),
                    unit.region.wasm_bytes(memory).is_ok(),
                )
            })
            .collect(),
    }
}

fn ram(memory: &AddressSpace, pages: u32) -> Vec<u8> {
    let mut bytes = vec![0; pages as usize * 4096];
    memory.read(GuestAddress(0x1000), &mut bytes).unwrap();
    bytes
}

#[test]
fn discard_current_reclaims_exact_default_credit_and_preserves_current_and_stale_buffers() {
    for target_index in [0, 3, 7] {
        let mut memory = fresh_memory(8);
        let mut registry = ResidentRegistry::new(&memory, RegistryLimits::default()).unwrap();
        let ids: Vec<_> = (0..8)
            .map(|page| compile(&mut registry, &memory, 0x1000 + page * 4096))
            .collect();
        let target = ids[target_index];
        let pc = 0x1000 + target_index as u32 * 4096;
        memory.write(GuestAddress(0x2000), &[0x90]).unwrap();
        assert_eq!(
            registry.get(&memory, ids[1]).unwrap_err(),
            RegistryError::CodeInvalidated
        );
        let before = saved(&registry, &memory);
        let before_ram = ram(&memory, 8);
        let identity = memory.identity();
        let snapshot = memory
            .snapshot_code(GuestAddress(0x1000), 8 * 4096)
            .unwrap();
        assert_eq!(
            registry.compile(
                &memory,
                &[BlockSpec {
                    entry: GuestAddress(pc),
                    byte_length: 3,
                }],
                CompileLimits::default(),
            ),
            Err(RegistryError::UnitCapacity)
        );
        assert_eq!(
            registry.retire_stale(&memory, target),
            Err(RegistryError::CurrentUnit)
        );
        assert_eq!(saved(&registry, &memory), before);

        registry.discard_current_raw(&memory, target.get()).unwrap();

        let mut expected = before;
        let removed = expected.units.remove(target_index);
        expected.usage.units -= 1;
        expected.usage.wasm_bytes -= removed.2.len();
        assert_eq!(saved(&registry, &memory), expected);
        assert_eq!(ram(&memory, 8), before_ram);
        assert_eq!(memory.identity(), identity);
        assert!(memory.is_code_current(&snapshot));
        assert_eq!(
            registry.get(&memory, target).unwrap_err(),
            RegistryError::InvalidUnit
        );
        let fresh = compile(&mut registry, &memory, pc);
        assert!(fresh.get() > target.get() && !ids.contains(&fresh));
        assert_eq!(registry.usage().units, 8);
        assert_eq!(registry.lookup(&memory, GuestAddress(pc)), Ok(fresh));
        assert_eq!(
            registry.discard_current_raw(&memory, target.get()),
            Err(RegistryError::InvalidUnit)
        );
        assert_eq!(
            &saved(&registry, &memory).units[..7],
            expected.units.as_slice()
        );
    }
}

#[test]
fn tight_byte_budget_is_reusable_after_current_discard_without_code_invalidation() {
    let memory = fresh_memory(1);
    let specs = [BlockSpec {
        entry: GuestAddress(0x1000),
        byte_length: 3,
    }];
    let bytes = compile_region(&memory, &specs, CompileLimits::default())
        .unwrap()
        .wasm_bytes(&memory)
        .unwrap()
        .len();
    // this unbound registry proof isolates byte credit from baked-id encoding size.
    let mut registry = ResidentRegistry::new(
        &memory,
        RegistryLimits {
            units: 8,
            wasm_bytes: bytes * 2 - 1,
        },
    )
    .unwrap();
    let old = compile(&mut registry, &memory, 0x1000);
    let before = saved(&registry, &memory);
    assert_eq!(
        registry.compile(&memory, &specs, CompileLimits::default()),
        Err(RegistryError::ByteCapacity)
    );
    assert_eq!(
        registry.retire_stale(&memory, old),
        Err(RegistryError::CurrentUnit)
    );
    assert_eq!(saved(&registry, &memory), before);
    let snapshot = memory.snapshot_code(GuestAddress(0x1000), 4096).unwrap();
    registry.discard_current_raw(&memory, old.get()).unwrap();
    assert_eq!(
        registry.usage(),
        RegistryUsage {
            units: 0,
            wasm_bytes: 0
        }
    );
    assert!(memory.is_code_current(&snapshot));
    let fresh = compile(&mut registry, &memory, 0x1000);
    assert!(fresh.get() > old.get());
    assert_eq!(
        registry.usage(),
        RegistryUsage {
            units: 1,
            wasm_bytes: bytes
        }
    );
}

#[test]
fn wrong_memory_unknown_stale_and_repeat_discard_preserve_registry_order_and_buffers() {
    let mut memory = fresh_memory(3);
    let wrong_memory = fresh_memory(1);
    let mut registry = ResidentRegistry::new(&memory, RegistryLimits::default()).unwrap();
    let target = compile(&mut registry, &memory, 0x1000);
    let stale = compile(&mut registry, &memory, 0x2000);
    compile(&mut registry, &memory, 0x3000);
    memory.write(GuestAddress(0x2000), &[0x90]).unwrap();
    let before = saved(&registry, &memory);
    for id in [0, target.get(), stale.get(), u64::MAX] {
        assert_eq!(
            registry.discard_current_raw(&wrong_memory, id),
            Err(RegistryError::WrongAddressSpace)
        );
        assert_eq!(saved(&registry, &memory), before);
    }
    // high-limb inputs are malformed identities, not fabricated valid large ids.
    for id in [0, target.get() ^ (1_u64 << 32), u64::MAX] {
        assert_eq!(
            registry.discard_current_raw(&memory, id),
            Err(RegistryError::InvalidUnit)
        );
        assert_eq!(saved(&registry, &memory), before);
    }
    assert_eq!(
        registry.discard_current_raw(&memory, stale.get()),
        Err(RegistryError::CodeInvalidated)
    );
    assert_eq!(saved(&registry, &memory), before);
    registry.discard_current_raw(&memory, target.get()).unwrap();
    let after = saved(&registry, &memory);
    assert_eq!(
        registry.discard_current_raw(&memory, target.get()),
        Err(RegistryError::InvalidUnit)
    );
    assert_eq!(saved(&registry, &memory), after);
}

#[test]
fn invalid_internal_credit_refuses_before_current_target_removal() {
    let memory = fresh_memory(2);
    let mut registry = ResidentRegistry::new(&memory, RegistryLimits::default()).unwrap();
    let target = compile(&mut registry, &memory, 0x1000);
    compile(&mut registry, &memory, 0x2000);
    let original_usage = registry.usage();
    let removed_bytes = registry.entries[0].region.retained_wasm_byte_len();
    // private fault injection preserves the deliberately inconsistent ledger on refusal.
    registry.wasm_bytes = removed_bytes - 1;
    let before = saved(&registry, &memory);
    assert_eq!(
        registry.discard_current_raw(&memory, target.get()),
        Err(RegistryError::InvalidLimits)
    );
    assert_eq!(saved(&registry, &memory), before);
    registry.get(&memory, target).unwrap();
    registry.wasm_bytes = original_usage.wasm_bytes;
    registry.discard_current_raw(&memory, target.get()).unwrap();
    assert_eq!(
        registry.usage().wasm_bytes,
        original_usage.wasm_bytes - removed_bytes
    );
    assert_eq!(registry.usage().units, 1);
    assert_eq!(saved(&registry, &memory).units, before.units[1..]);
}
