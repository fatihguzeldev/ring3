use super::*;
use crate::memory::{PageRange, Permissions};

fn fixture() -> (AddressSpace, ResidentRegistry) {
    let mut memory = AddressSpace::new(8).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 8).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    for page in 0..8 {
        memory
            .write(GuestAddress(0x1000 + page * 4096), &[0x90, 0xeb, 0])
            .unwrap();
    }
    let registry = ResidentRegistry::new(&memory, RegistryLimits::default()).unwrap();
    (memory, registry)
}

fn compile(registry: &mut ResidentRegistry, memory: &AddressSpace, id: u64, pc: u32) -> UnitId {
    let actual = registry
        .compile_bound_with_counter(
            memory,
            &[BlockSpec {
                entry: GuestAddress(pc),
                byte_length: 3,
            }],
            &[],
            CompileLimits::default(),
            7,
            None,
            crate::cpu::x86::decode::DecodeAuthority::default(),
            &AtomicU64::new(id),
        )
        .unwrap();
    assert_eq!(actual.get(), id);
    actual
}

fn snapshot(
    registry: &ResidentRegistry,
    memory: &AddressSpace,
    id: UnitId,
) -> (*const u8, Vec<u8>) {
    let bytes = registry
        .get(memory, id)
        .unwrap()
        .wasm_bytes(memory)
        .unwrap();
    (bytes.as_ptr(), bytes.to_vec())
}

fn assert_snapshot(
    registry: &ResidentRegistry,
    memory: &AddressSpace,
    id: UnitId,
    expected: &(*const u8, Vec<u8>),
) {
    let bytes = registry
        .get_raw(memory, id.get())
        .unwrap()
        .wasm_bytes(memory)
        .unwrap();
    assert_eq!(bytes.as_ptr(), expected.0);
    assert_eq!(bytes, expected.1);
}

#[test]
fn warmed_index_hints_survive_removal_shifts_and_out_of_bounds_indices() {
    let (memory, mut registry) = fixture();
    let ids = std::array::from_fn::<_, 4, _>(|index| {
        compile(
            &mut registry,
            &memory,
            index as u64 + 1,
            0x1000 + index as u32 * 4096,
        )
    });
    let expected = ids.map(|id| snapshot(&registry, &memory, id));
    registry
        .discard_quiescent_raw(&memory, ids[0].get())
        .unwrap();
    // the former last index is now outside the vector, and middle indices name other ids.
    for index in [3, 1, 2] {
        assert_snapshot(&registry, &memory, ids[index], &expected[index]);
    }
    registry
        .discard_quiescent_raw(&memory, ids[2].get())
        .unwrap();
    assert_snapshot(&registry, &memory, ids[3], &expected[3]);
    assert_snapshot(&registry, &memory, ids[1], &expected[1]);
    for id in [ids[0], ids[2]] {
        assert_eq!(
            registry.get(&memory, id).unwrap_err(),
            RegistryError::InvalidUnit
        );
    }
    let newest = compile(&mut registry, &memory, 5, 0x1000);
    assert!(registry.get(&memory, newest).is_ok());
    assert_snapshot(&registry, &memory, ids[3], &expected[3]);
    assert_snapshot(&registry, &memory, ids[1], &expected[1]);
}

#[test]
fn index_hints_recheck_full_ids_for_collisions_and_preserve_invalid_lookups() {
    let (memory, mut registry) = fixture();
    let ids = [1, 65, (1 << 63) | 1].map(|id| {
        let page = registry.usage().units as u32;
        compile(&mut registry, &memory, id, 0x1000 + page * 4096)
    });
    let expected = ids.map(|id| snapshot(&registry, &memory, id));
    let usage = registry.usage();
    for index in [0, 1, 2, 1, 0, 2] {
        assert_snapshot(&registry, &memory, ids[index], &expected[index]);
        for absent in [0, 129, (1 << 63) | 65, u64::MAX] {
            assert_eq!(
                registry.get_raw(&memory, absent).unwrap_err(),
                RegistryError::InvalidUnit
            );
            assert_eq!(
                registry.get_retained_raw(&memory, absent).unwrap_err(),
                RegistryError::InvalidUnit
            );
        }
        assert_snapshot(&registry, &memory, ids[index], &expected[index]);
        assert_eq!(registry.usage(), usage);
    }
}

#[test]
fn warmed_index_hint_preserves_source_validation_and_memory_error_precedence() {
    let (mut memory, mut registry) = fixture();
    let id = compile(&mut registry, &memory, 1, 0x1000);
    let expected = snapshot(&registry, &memory, id);
    memory.write(GuestAddress(0x1000), &[0x40]).unwrap();
    assert_eq!(
        registry.get_raw(&memory, id.get()).unwrap_err(),
        RegistryError::CodeInvalidated
    );
    let retained = registry.get_retained_raw(&memory, id.get()).unwrap();
    assert_eq!(retained.retained_wasm_bytes_for_test().as_ptr(), expected.0);
    assert_eq!(retained.retained_wasm_bytes_for_test(), expected.1);
    let other = AddressSpace::new(8).unwrap();
    assert_eq!(
        registry.get_raw(&other, id.get()).unwrap_err(),
        RegistryError::WrongAddressSpace
    );
    assert_eq!(
        registry.get_retained_raw(&other, u64::MAX).unwrap_err(),
        RegistryError::WrongAddressSpace
    );
    assert_eq!(
        registry.discard_current_raw(&memory, id.get()),
        Err(RegistryError::CodeInvalidated)
    );
    registry.retire_stale(&memory, id).unwrap();
    assert_eq!(
        registry.get_retained_raw(&memory, id.get()).unwrap_err(),
        RegistryError::InvalidUnit
    );
}

#[test]
fn registry_index_hints_preserve_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<ResidentRegistry>();
}
