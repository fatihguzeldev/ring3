use super::*;
use crate::memory::{PageRange, Permissions};

fn fixture() -> (AddressSpace, ResidentRegistry, AtomicU64) {
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
    (memory, registry, AtomicU64::new((1 << 63) - 3))
}

fn compile(
    registry: &mut ResidentRegistry,
    memory: &AddressSpace,
    counter: &AtomicU64,
    pc: u32,
) -> UnitId {
    registry
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
            counter,
        )
        .unwrap()
}

#[test]
fn sorted_identity_lookup_survives_unsigned_ids_removal_holes_and_new_publication() {
    let (memory, mut registry, counter) = fixture();
    let ids = (0..6)
        .map(|page| compile(&mut registry, &memory, &counter, 0x1000 + page * 4096))
        .collect::<Vec<_>>();
    assert!(ids.windows(2).all(|pair| pair[0].get() < pair[1].get()));
    assert!(ids[2].get() < (1 << 63) && ids[3].get() >= (1 << 63));
    let kept = [1, 3, 4].map(|index| {
        let region = registry.get(&memory, ids[index]).unwrap();
        (
            ids[index],
            region.wasm_bytes(&memory).unwrap().as_ptr(),
            region.wasm_bytes(&memory).unwrap().to_vec(),
        )
    });
    for index in [0, 2, 5] {
        registry
            .discard_quiescent_raw(&memory, ids[index].get())
            .unwrap();
    }
    let newest = compile(&mut registry, &memory, &counter, 0x1000);
    assert!(newest.get() > ids[5].get());
    assert!(
        registry
            .entries
            .windows(2)
            .all(|pair| pair[0].id.get() < pair[1].id.get())
    );
    for (id, pointer, bytes) in kept {
        let actual = registry
            .get_raw(&memory, id.get())
            .unwrap()
            .wasm_bytes(&memory)
            .unwrap();
        assert_eq!(actual.as_ptr(), pointer);
        assert_eq!(actual, bytes);
        assert_eq!(
            registry
                .get_retained_raw(&memory, id.get())
                .unwrap()
                .wasm_bytes(&memory)
                .unwrap(),
            bytes
        );
    }
    for absent in [0, ids[0].get(), ids[2].get(), ids[5].get(), u64::MAX] {
        assert_eq!(
            registry.get_raw(&memory, absent).unwrap_err(),
            RegistryError::InvalidUnit
        );
        assert_eq!(
            registry.get_retained_raw(&memory, absent).unwrap_err(),
            RegistryError::InvalidUnit
        );
    }
    assert!(registry.get(&memory, newest).is_ok());
}

#[test]
fn identity_lookup_preserves_stale_retained_views_and_wrong_memory_error_precedence() {
    let (mut memory, mut registry, counter) = fixture();
    let id = compile(&mut registry, &memory, &counter, 0x1000);
    let bytes = registry
        .get(&memory, id)
        .unwrap()
        .wasm_bytes(&memory)
        .unwrap()
        .to_vec();
    memory.write(GuestAddress(0x1000), &[0x40]).unwrap();
    assert_eq!(
        registry.get_raw(&memory, id.get()).unwrap_err(),
        RegistryError::CodeInvalidated
    );
    let retained = registry.get_retained_raw(&memory, id.get()).unwrap();
    assert_eq!(retained.retained_wasm_bytes_for_test(), bytes);
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
