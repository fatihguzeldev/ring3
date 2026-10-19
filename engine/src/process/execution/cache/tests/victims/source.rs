use super::*;
use crate::{
    cpu::dbt::{BlockSpec, CompileLimits, RegistryLimits, ResidentRegistry},
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

#[test]
fn fresh_rw_skips_real_resident_validity_rescan_and_source_changes_restore_stale_first() {
    let mut memory = AddressSpace::new(4).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 3).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    for pc in [0x1000, 0x2000, 0x3000] {
        memory.write(GuestAddress(pc), &[0x90, 0xeb, 0xfe]).unwrap();
    }
    let mut registry = ResidentRegistry::new(&memory, RegistryLimits::default()).unwrap();
    let ids = [0x1000, 0x2000, 0x3000].map(|pc| {
        registry
            .compile(
                &memory,
                &[BlockSpec {
                    entry: GuestAddress(pc),
                    byte_length: 3,
                }],
                CompileLimits::default(),
            )
            .unwrap()
            .get()
    });
    let entries = ids
        .iter()
        .enumerate()
        .map(|(index, &id)| Some(row(id, index as u64 + 1)))
        .collect::<Vec<_>>();
    let mut victims = VictimIndex::new(3).unwrap();
    for (slot, &id) in ids.iter().enumerate() {
        victims.install(slot as u32, id, slot as u64 + 1);
    }
    let checks = Cell::new(0);
    let select = |victims: &mut VictimIndex, memory: &AddressSpace| {
        victims.select(
            &entries,
            memory.code_source_version(),
            |id| {
                checks.set(checks.get() + 1);
                registry.get_raw(memory, id).is_ok()
            },
            &[],
        )
    };
    assert_eq!(select(&mut victims, &memory), Some(0));
    assert_eq!(checks.replace(0), 3);
    let source = memory.code_source_version();
    let transaction = memory.transaction_version();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x4000), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    assert_eq!(memory.code_validation_version(), transaction + 1);
    assert_eq!(memory.code_source_version(), source);
    assert_eq!(select(&mut victims, &memory), Some(0));
    assert_eq!(
        checks.replace(0),
        0,
        "fresh private rw avoids every real registry currentness call"
    );
    memory
        .protect(
            PageRange::new(GuestAddress(0x4000), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    assert_eq!(select(&mut victims, &memory), Some(0));
    assert_eq!(
        checks.replace(0),
        3,
        "even an unrelated protect keeps the conservative refresh contract"
    );
    memory.write(GuestAddress(0x3000), &[0x91]).unwrap();
    assert_eq!(
        select(&mut victims, &memory),
        Some(2),
        "newest stale owner precedes the oldest current owner"
    );
    assert_eq!(checks.get(), 3);
}
