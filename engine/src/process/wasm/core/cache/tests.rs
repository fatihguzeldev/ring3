use super::*;
use crate::{
    cpu::core::{CompileOptions, compile},
    memory::{GuestAddress, PageRange, Permissions},
};

fn memory() -> AddressSpace {
    let filler_bytes = (MAX_REGIONS + 64) * 8;
    let filler_pages = filler_bytes.div_ceil(4096);
    let mut memory = AddressSpace::new((2 + filler_pages) as u32).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x3000), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    let mut code = [0; 4096];
    code[..15].copy_from_slice(&[
        0x83, 0xc0, 1, 0x75, 5, 0x83, 0xc0, 2, 0xeb, 3, 0x83, 0xc0, 3, 0x0f, 0x0b,
    ]);
    memory.write(GuestAddress(0x1000), &code).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x4000), filler_pages as u32).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    let mut fillers = vec![0; filler_pages * 4096];
    for index in 0..MAX_REGIONS + 64 {
        fillers[index * 8..index * 8 + 3].copy_from_slice(&[0x90, 0x0f, 0x0b]);
    }
    memory.write(GuestAddress(0x4000), &fillers).unwrap();
    memory
}

fn compiled(memory: &AddressSpace, pc: u32) -> CompiledCode {
    compile(
        memory,
        GuestAddress(pc),
        CompileOptions {
            instructions: 32,
            blocks: 8,
            ..CompileOptions::default()
        },
    )
    .unwrap()
}

fn filler(index: usize) -> u32 {
    0x4000 + index as u32 * 8
}

fn same(left: &RegionRef, right: &RegionRef) -> bool {
    Rc::ptr_eq(&left.code, &right.code)
}

fn bounds(cache: &CoreCache32) {
    let count = cache.regions.len() - cache.free.len();
    assert!(cache.regions.len() <= MAX_REGION_EXTENT);
    assert!(cache.bytes <= MAX_CODE_BYTES);
    assert!(cache.resident_charge() <= MAX_CODE_BYTES);
    assert_eq!(cache.snapshot()[8], cache.resident_charge() as u64);
    assert_eq!(cache.snapshot()[9], MAX_CODE_BYTES as u64);
    assert_eq!(cache.snapshot()[10], cache.link_capacity() as u64);
    assert_eq!(
        cache.owner_charge,
        cache
            .regions
            .iter()
            .flatten()
            .map(|entry| entry.charge)
            .sum::<usize>()
    );
    assert_eq!(
        cache.bytes,
        cache
            .regions
            .iter()
            .flatten()
            .map(|entry| entry.code.wasm().len())
            .sum::<usize>()
    );
    assert!(
        cache
            .entries
            .values()
            .all(|key| cache.region(*key).is_some())
    );
    let mut seen = std::collections::HashSet::new();
    let (mut previous, mut current) = (None, cache.oldest());
    while let Some(index) = current {
        assert!(seen.insert(index), "LRU has a cycle");
        assert!(cache.regions[index].is_some());
        let entry = &cache.route_owners[index];
        assert_eq!(CoreCache32::route_index(entry.older), previous);
        previous = Some(index);
        current = CoreCache32::route_index(entry.newer);
    }
    assert_eq!(previous, cache.newest());
    assert_eq!(seen.len(), count);
    let free: std::collections::HashSet<_> = cache.free.iter().copied().collect();
    assert_eq!(free.len(), cache.free.len());
    for index in 0..cache.regions.len() {
        assert_eq!(cache.regions[index].is_none(), free.contains(&index));
    }
    assert_eq!(cache.link_owners[0], 0);
    assert_eq!(cache.slot_regions[0], NO_REGION);
    for (index, entry) in cache.regions.iter().enumerate() {
        if let Some(entry) = entry.as_ref() {
            let key = RegionKey {
                index,
                identity: entry.code.identity(),
            };
            assert_ne!(entry.indexed_entries, 0);
            assert_eq!(
                entry.indexed_entries,
                entry
                    .admitted_entries
                    .iter()
                    .filter(|pc| cache.entries.get(&pc.0) == Some(&key))
                    .count()
            );
            if entry.slot != 0 {
                assert!(cache.reciprocal_slot(key, entry));
            }
        }
    }
    for (slot, &identity) in cache.link_owners.iter().enumerate().skip(1) {
        if identity == 0 {
            assert_eq!(cache.slot_regions[slot], NO_REGION);
        } else {
            let key = RegionKey {
                index: cache.slot_regions[slot] as usize,
                identity,
            };
            assert_eq!(cache.region(key).unwrap().slot as usize, slot);
        }
    }
}

#[test]
fn source_changes_and_identical_remapping_invalidate_cached_entries() {
    let mut memory = memory();
    let mut cache = CoreCache32::default();
    let first = cache.insert(compiled(&memory, 0x1000));
    memory.write(GuestAddress(0x3000), &[7]).unwrap();
    assert!(same(&cache.lookup(&memory, 0x1000).unwrap(), &first));
    memory.write(GuestAddress(0x1002), &[4]).unwrap();
    assert!(cache.lookup(&memory, 0x1000).is_none());
    assert!(cache.free.len() == cache.regions.len());
    for entry in first.entrypoints() {
        assert!(!cache.entries.contains_key(&entry.0));
    }

    let second = cache.insert(compiled(&memory, 0x1000));
    let mut code = [0; 4096];
    memory.read(GuestAddress(0x1000), &mut code).unwrap();
    let range = PageRange::new(GuestAddress(0x1000), 1).unwrap();
    memory.unmap(range).unwrap();
    memory.map_zeroed(range, Permissions::ALL).unwrap();
    memory.write(GuestAddress(0x1000), &code).unwrap();
    assert!(!second.is_current(&memory));
    assert!(cache.lookup(&memory, 0x1005).is_none());
    assert!(cache.free.len() == cache.regions.len());
    bounds(&cache);
}

#[test]
fn actual_compiled_side_entries_reuse_the_same_retained_region() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let code = cache.insert(compiled(&memory, 0x1000));
    assert_eq!(
        code.entrypoints(),
        &[
            GuestAddress(0x1000),
            GuestAddress(0x1005),
            GuestAddress(0x100a),
            GuestAddress(0x100d)
        ]
    );
    for pc in [0x1000, 0x1005, 0x100a, 0x100d] {
        assert!(same(&cache.lookup(&memory, pc).unwrap(), &code));
    }
    assert!(cache.lookup(&memory, 0x1001).is_none());
    assert_eq!((cache.regions.len() - cache.free.len()), 1);
    bounds(&cache);
}

#[test]
fn pc_lookup_preserves_distinct_owners_with_colliding_hash_prefixes() {
    use std::hash::BuildHasher;

    let memory = memory();
    let mut cache = CoreCache32::default();
    cache.entries.reserve(64);
    let mut buckets: [Vec<u32>; 128] = std::array::from_fn(|_| Vec::new());
    for index in 0..MAX_REGIONS {
        let pc = filler(index);
        let bucket = (cache.entries.hasher().hash_one(pc) & 127) as usize;
        buckets[bucket].push(pc);
    }
    let pcs: Vec<_> = buckets
        .into_iter()
        .max_by_key(Vec::len)
        .unwrap()
        .into_iter()
        .take(32)
        .collect();
    assert_eq!(pcs.len(), 32);
    let owners: Vec<_> = pcs
        .iter()
        .enumerate()
        .map(|(index, &pc)| {
            let code = cache.insert(compiled(&memory, pc));
            cache.install_link(&code, index as u32 + 1).unwrap();
            code
        })
        .collect();
    for (index, (&pc, owner)) in pcs.iter().zip(&owners).enumerate() {
        assert!(same(&cache.lookup(&memory, pc).unwrap(), owner));
        let (linked, slot) = cache.lookup_linked(&memory, pc).unwrap();
        assert!(same(&linked, owner));
        assert_eq!(slot, index as u32 + 1);
    }
    assert!(cache.lookup(&memory, 0x1234_5678).is_none());

    cache.remove(owners[0].key);
    assert!(cache.lookup(&memory, pcs[0]).is_none());
    assert!(cache.lookup_linked(&memory, pcs[0]).is_none());
    assert!(owners[0].is_current(&memory));
    let replacement = cache.insert(compiled(&memory, pcs[0]));
    assert_ne!(replacement.identity(), owners[0].identity());
    cache.install_link(&replacement, 1).unwrap();
    assert!(same(&cache.lookup(&memory, pcs[0]).unwrap(), &replacement));
    assert!(cache.retained(&owners[0]).is_none());
    for (&pc, owner) in pcs.iter().zip(&owners).skip(1) {
        assert!(same(&cache.lookup_linked(&memory, pc).unwrap().0, owner));
    }
    bounds(&cache);
}

#[test]
fn lru_touch_selects_the_untouched_owner_for_explicit_eviction() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let first = cache.insert(compiled(&memory, filler(0)));
    let second = cache.insert(compiled(&memory, filler(1)));
    let third = cache.insert(compiled(&memory, filler(2)));
    assert!(same(&cache.lookup(&memory, filler(0)).unwrap(), &first));
    assert_eq!(cache.oldest(), Some(second.key.index));
    cache.remove(second.key);
    assert!(cache.lookup(&memory, filler(1)).is_none());
    assert!(same(&cache.lookup(&memory, filler(0)).unwrap(), &first));
    assert!(same(&cache.lookup(&memory, filler(2)).unwrap(), &third));
    assert_eq!(cache.snapshot()[2..5], [0, 0, 0]);
    bounds(&cache);
}

#[test]
fn newer_side_entry_alias_survives_explicit_removal_of_the_previous_region() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let old = cache.insert(compiled(&memory, 0x1000));
    let newer = cache.insert(compiled(&memory, 0x1005));
    assert_ne!(old.identity(), newer.identity());
    assert_eq!(
        newer.entrypoints(),
        &[GuestAddress(0x1005), GuestAddress(0x100d)]
    );
    cache.remove(old.key);
    assert!(cache.retained(&old).is_none());
    assert!(cache.lookup(&memory, 0x1000).is_none());
    assert!(cache.lookup(&memory, 0x100a).is_none());
    for pc in [0x1005, 0x100d] {
        assert!(same(&cache.lookup(&memory, pc).unwrap(), &newer));
    }
    bounds(&cache);
}

#[test]
fn partially_shadowed_owners_retain_their_other_entries_and_links() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let old = cache.insert(compiled(&memory, 0x1000));
    cache.install_link(&old, 1).unwrap();
    let newer = cache.insert(compiled(&memory, 0x1005));
    cache.install_link(&newer, 2).unwrap();
    assert_eq!(cache.retained(&old).unwrap().indexed_entries, 2);
    assert_eq!(cache.retained(&newer).unwrap().indexed_entries, 2);
    for pc in [0x1000, 0x100a] {
        let (selected, slot) = cache.lookup_linked(&memory, pc).unwrap();
        assert!(same(&selected, &old));
        assert_eq!(slot, 1);
    }
    for pc in [0x1005, 0x100d] {
        let (selected, slot) = cache.lookup_linked(&memory, pc).unwrap();
        assert!(same(&selected, &newer));
        assert_eq!(slot, 2);
    }
    assert_eq!(cache.snapshot()[2..6], [0, 0, 0, 2]);
    bounds(&cache);
}

#[test]
fn losing_the_last_index_reclaims_every_displaced_owner_but_not_held_code() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let old = cache.insert(compiled(&memory, 0x1000));
    cache.install_link(&old, 1).unwrap();
    let partial = cache.insert(compiled(&memory, 0x1005));
    cache.install_link(&partial, 2).unwrap();
    let newer = cache.insert(compiled(&memory, 0x1000));
    assert!(cache.retained(&old).is_none());
    assert!(cache.retained(&partial).is_none());
    for slot in [1, 2] {
        assert_eq!(cache.link_owners[slot], 0);
        assert_eq!(cache.slot_regions[slot], NO_REGION);
    }
    for held in [&old, &partial] {
        assert!(held.is_current(&memory));
        assert!(!held.wasm().is_empty());
        assert!(cache.install_link(held, 1).is_err());
    }
    cache.install_link(&newer, 1).unwrap();
    assert!(cache.unlink(1, old.identity()).is_err());
    for pc in [0x1000, 0x1005, 0x100a, 0x100d] {
        let (selected, slot) = cache.lookup_linked(&memory, pc).unwrap();
        assert!(same(&selected, &newer));
        assert_eq!(slot, 1);
    }
    assert_eq!(
        cache.snapshot()[..8],
        [
            3,
            (old.wasm().len() + partial.wasm().len() + newer.wasm().len()) as u64,
            0,
            0,
            0,
            1,
            newer.wasm().len() as u64,
            MAX_REGIONS as u64,
        ]
    );
    bounds(&cache);
}

#[test]
fn links_require_a_retained_owner_and_accept_the_last_driver_slot() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let code = cache.insert(compiled(&memory, 0x1000));
    let outside = CoreCache32::default().insert(compiled(&memory, filler(0)));
    assert!(cache.install_link(&code, 0).is_err());
    let last = cache.link_capacity() as u32;
    assert!(cache.install_link(&code, last + 1).is_err());
    assert!(cache.install_link(&outside, 1).is_err());
    assert!(cache.lookup_linked(&memory, 0x1000).is_none());
    cache.install_link(&code, last).unwrap();
    cache.install_link(&code, last).unwrap();
    assert!(cache.install_link(&code, 1).is_err());
    let (selected, slot) = cache.lookup_linked(&memory, 0x1005).unwrap();
    assert!(same(&selected, &code));
    assert_eq!(slot, last);
    bounds(&cache);
}

#[test]
fn occupied_and_wrong_owner_link_operations_leave_both_owners_unchanged() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let first = cache.insert(compiled(&memory, filler(0)));
    let second = cache.insert(compiled(&memory, filler(1)));
    cache.install_link(&first, 1).unwrap();
    assert!(cache.install_link(&second, 1).is_err());
    cache.install_link(&second, 2).unwrap();
    assert!(cache.unlink(1, second.identity()).is_err());
    assert!(cache.unlink(0, first.identity()).is_err());
    assert!(
        cache
            .unlink(cache.link_capacity() as u32 + 1, first.identity())
            .is_err()
    );
    for (pc, code, expected) in [(filler(0), &first, 1), (filler(1), &second, 2)] {
        let (selected, slot) = cache.lookup_linked(&memory, pc).unwrap();
        assert!(same(&selected, code));
        assert_eq!(slot, expected);
    }
    bounds(&cache);
}

#[test]
fn unlink_is_idempotent_for_an_empty_slot_and_preserves_cached_code() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let code = cache.insert(compiled(&memory, filler(0)));
    cache.install_link(&code, 1).unwrap();
    cache.unlink(1, code.identity()).unwrap();
    cache.unlink(1, code.identity()).unwrap();
    assert!(cache.lookup_linked(&memory, filler(0)).is_none());
    assert!(same(&cache.lookup(&memory, filler(0)).unwrap(), &code));
    cache.install_link(&code, 2).unwrap();
    assert_eq!(cache.lookup_linked(&memory, filler(0)).unwrap().1, 2);
    bounds(&cache);
}

#[test]
fn explicit_removal_releases_the_link_but_a_held_current_region_cannot_reinstall_it() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let old = cache.insert(compiled(&memory, filler(0)));
    cache.install_link(&old, 1).unwrap();
    cache.remove(old.key);
    assert!(old.is_current(&memory));
    assert!(!old.wasm().is_empty());
    assert!(cache.lookup_linked(&memory, filler(0)).is_none());
    assert!(cache.install_link(&old, 1).is_err());
    let newest = cache.insert(compiled(&memory, filler(1)));
    cache.install_link(&newest, 1).unwrap();
    assert!(cache.unlink(1, old.identity()).is_err());
    let (selected, slot) = cache.lookup_linked(&memory, filler(1)).unwrap();
    assert!(same(&selected, &newest));
    assert_eq!(slot, 1);
    bounds(&cache);
}

#[test]
fn a_linked_source_change_rejects_and_releases_the_slot_for_new_code() {
    let mut memory = memory();
    let mut cache = CoreCache32::default();
    let old = cache.insert(compiled(&memory, 0x1000));
    cache.install_link(&old, 1).unwrap();
    memory.write(GuestAddress(0x1002), &[4]).unwrap();
    assert!(cache.lookup_linked(&memory, 0x1000).is_none());
    assert!(cache.lookup_linked(&memory, 0x1005).is_none());
    let fresh = cache.insert(compiled(&memory, 0x1000));
    cache.install_link(&fresh, 1).unwrap();
    assert!(cache.unlink(1, old.identity()).is_err());
    let (selected, slot) = cache.lookup_linked(&memory, 0x1000).unwrap();
    assert!(same(&selected, &fresh));
    assert_eq!(slot, 1);
    bounds(&cache);
}

#[test]
fn linked_lookup_is_source_checked_and_touches_the_same_lru() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let first = cache.insert(compiled(&memory, filler(0)));
    let second = cache.insert(compiled(&memory, filler(1)));
    cache.install_link(&first, 1).unwrap();
    cache.install_link(&second, 2).unwrap();
    cache.insert(compiled(&memory, filler(2)));
    assert!(cache.lookup_linked(&memory, 0xdead_0000).is_none());
    assert!(same(
        &cache.lookup_linked(&memory, filler(0)).unwrap().0,
        &first
    ));
    assert_eq!(cache.oldest(), Some(second.key.index));
    cache.remove(second.key);
    let newest = cache.insert(compiled(&memory, filler(3)));
    assert!(cache.lookup_linked(&memory, filler(1)).is_none());
    assert!(cache.install_link(&second, 2).is_err());
    cache.install_link(&newest, 2).unwrap();
    assert_eq!(cache.lookup_linked(&memory, filler(3)).unwrap().1, 2);
    assert_eq!(cache.lookup_linked(&memory, filler(0)).unwrap().1, 1);
    bounds(&cache);
}

#[test]
fn more_than_256_small_regions_remain_cached_below_the_byte_limit() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let first = cache.insert(compiled(&memory, filler(0)));
    cache.install_link(&first, 1).unwrap();
    for index in 1..300 {
        cache.insert(compiled(&memory, filler(index)));
    }
    assert_eq!((cache.regions.len() - cache.free.len()), 300);
    assert!(cache.bytes < MAX_CODE_BYTES);
    assert_eq!(
        cache.snapshot()[..8],
        [
            300,
            cache.bytes as u64,
            0,
            0,
            0,
            300,
            cache.bytes as u64,
            MAX_REGIONS as u64
        ]
    );
    let (selected, slot) = cache.lookup_linked(&memory, filler(0)).unwrap();
    assert!(same(&selected, &first));
    assert_eq!(slot, 1);
    bounds(&cache);
}

#[test]
fn a_thousand_twenty_five_regions_retain_both_endpoints_below_the_byte_budget() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let first = cache.insert(compiled(&memory, filler(0)));
    cache.install_link(&first, 1).unwrap();
    let mut last = first.clone();
    for index in 1..=1024 {
        last = cache.insert(compiled(&memory, filler(index)));
    }
    cache.install_link(&last, 1025).unwrap();
    assert_eq!(cache.regions.len() - cache.free.len(), 1025);
    assert!(cache.bytes < MAX_CODE_BYTES);
    assert_eq!(cache.snapshot()[2..5], [0, 0, 0]);
    let (retained_first, first_slot) = cache.lookup_linked(&memory, filler(0)).unwrap();
    let (retained_last, last_slot) = cache.lookup_linked(&memory, filler(1024)).unwrap();
    assert!(same(&retained_first, &first));
    assert!(same(&retained_last, &last));
    assert_eq!((first_slot, last_slot), (1, 1025));
    bounds(&cache);
}

#[test]
fn real_charged_byte_pressure_evicts_and_releases_links_before_initial_extent_is_exhausted() {
    let mut memory = AddressSpace::new(4).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 4).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory
        .write(GuestAddress(0x1000), &[0x83, 0x03, 1].repeat(4096))
        .unwrap();
    let limits = CompileOptions {
        instructions: 4096,
        blocks: 1,
        wasm_bytes: 16 << 20,
    };
    let mut cache = CoreCache32::default();
    let first = cache.insert(compile(&memory, GuestAddress(0x1000), limits).unwrap());
    let bytes = first.wasm().len();
    let count = MAX_CODE_BYTES / bytes + 1;
    assert!(
        count < MAX_REGIONS,
        "byte pressure must precede count pressure"
    );
    cache.install_link(&first, 1).unwrap();
    let mut newest = first.clone();
    let mut admitted_bytes = bytes;
    for index in 1..count {
        let pc = 0x1000 + index as u32 * 3;
        let code = compile(&memory, GuestAddress(pc), limits).unwrap();
        assert_eq!(code.entrypoints(), &[GuestAddress(pc)]);
        admitted_bytes += code.wasm().len();
        newest = cache.insert(code);
        cache.install_link(&newest, index as u32 + 1).unwrap();
        bounds(&cache);
    }
    assert!((cache.regions.len() - cache.free.len()) < count);
    assert_eq!(
        cache.snapshot()[..8],
        [
            count as u64,
            admitted_bytes as u64,
            0,
            (count - (cache.regions.len() - cache.free.len())) as u64,
            0,
            (cache.regions.len() - cache.free.len()) as u64,
            cache.bytes as u64,
            MAX_REGIONS as u64
        ]
    );
    assert!(!cache.retained(&first).is_some());
    assert!(cache.install_link(&first, 1).is_err());
    let oldest_index = cache.oldest().unwrap();
    let oldest = RegionKey {
        index: oldest_index,
        identity: cache.regions[oldest_index]
            .as_ref()
            .unwrap()
            .code
            .identity(),
    };
    let newest_pc = newest.entrypoints()[0];
    let mut replacement = compile(&memory, newest_pc, limits).unwrap();
    let replacement_bytes = replacement.wasm().len();
    // Set declared storage strictly inside the actual available/self-displaced
    // interval; generated metadata density must not decide this test's pressure.
    let wasm_capacity = replacement.set_wasm_capacity_for_test(replacement_bytes);
    let charge = CoreCache32::code_charge(&replacement, replacement.entrypoints().len());
    let slack = MAX_CODE_BYTES - cache.resident_charge();
    let displaced_charge = cache.retained(&newest).unwrap().charge;
    let target = (slack + displaced_charge / 2).max(charge);
    assert!(target > slack && target <= slack + displaced_charge);
    replacement.set_wasm_capacity_for_test(wasm_capacity + target - charge);
    assert!(
        cache.resident_charge()
            + CoreCache32::code_charge(&replacement, replacement.entrypoints().len())
            > MAX_CODE_BYTES
    );
    assert!(
        cache.resident_charge() - cache.retained(&newest).unwrap().charge
            + CoreCache32::code_charge(&replacement, replacement.entrypoints().len())
            <= MAX_CODE_BYTES
    );
    let pressure = cache.snapshot();
    let displaced = newest;
    let newest = cache.insert(replacement);
    assert!(
        cache.region(oldest).is_some(),
        "unrelated oldest owner survives"
    );
    assert!(cache.retained(&displaced).is_none());
    assert_eq!(cache.link_owners[count], 0);
    assert_eq!(cache.snapshot()[2..6], pressure[2..6]);
    assert_eq!(cache.snapshot()[0], pressure[0] + 1);
    assert_eq!(cache.snapshot()[1], pressure[1] + replacement_bytes as u64);
    assert_eq!(
        cache.snapshot()[6],
        pressure[6] - displaced.wasm().len() as u64 + replacement_bytes as u64
    );
    cache.install_link(&newest, count as u32).unwrap();
    cache.unlink(count as u32, newest.identity()).unwrap();
    cache.install_link(&newest, 1).unwrap();
    assert!(cache.unlink(1, first.identity()).is_err());
    assert_eq!(cache.lookup_linked(&memory, newest_pc.0).unwrap().1, 1);
    bounds(&cache);
}

#[test]
fn oldest_linked_slot_follows_native_hits_and_skips_unlinked_owners() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    assert_eq!(cache.oldest_linked_slot(), 0);
    let first = cache.insert(compiled(&memory, filler(0)));
    let second = cache.insert(compiled(&memory, filler(1)));
    cache.insert(compiled(&memory, filler(2)));
    assert_eq!(cache.oldest_linked_slot(), 0);
    cache.install_link(&first, 11).unwrap();
    cache.install_link(&second, 29).unwrap();
    assert_eq!(cache.oldest_linked_slot(), 11);

    cache.lookup_linked(&memory, filler(0)).unwrap();
    assert_eq!(cache.oldest_linked_slot(), 29);
    cache.lookup_linked(&memory, filler(1)).unwrap();
    assert_eq!(cache.oldest_linked_slot(), 11);
    cache.unlink(11, first.identity()).unwrap();
    assert_eq!(cache.oldest_linked_slot(), 29);
    cache.unlink(29, second.identity()).unwrap();
    assert_eq!(cache.oldest_linked_slot(), 0);
    bounds(&cache);
}

#[test]
fn cold_snapshot_counts_compilation_and_lazy_source_removal_without_counting_hits() {
    let mut memory = memory();
    let mut cache = CoreCache32::default();
    assert_eq!(
        cache.snapshot()[..8],
        [0, 0, 0, 0, 0, 0, 0, cache.regions.len() as u64]
    );
    let first = cache.insert(compiled(&memory, 0x1000));
    let second = cache.insert(compiled(&memory, filler(0)));
    let first_bytes = first.wasm().len() as u64;
    let second_bytes = second.wasm().len() as u64;
    let initial = [
        2,
        first_bytes + second_bytes,
        0,
        0,
        0,
        2,
        first_bytes + second_bytes,
        cache.regions.len() as u64,
    ];
    assert_eq!(cache.snapshot()[..8], initial);
    let unlinked = cache.snapshot();
    cache.install_link(&first, 1).unwrap();
    let mut linked = unlinked;
    linked[11] += 1;
    assert_eq!(cache.snapshot(), linked);
    for _ in 0..3 {
        cache.lookup(&memory, 0x1000).unwrap();
        cache.lookup_linked(&memory, 0x1005).unwrap();
        assert_eq!(cache.snapshot(), linked);
    }
    cache.unlink(1, first.identity()).unwrap();
    linked[11] += 1;
    assert_eq!(cache.snapshot(), linked);
    memory.write(GuestAddress(0x1002), &[4]).unwrap();
    assert!(cache.lookup(&memory, 0x1000).is_none());
    assert!(cache.lookup(&memory, 0x1005).is_none());
    assert_eq!(
        cache.snapshot()[..8],
        [
            2,
            first_bytes + second_bytes,
            0,
            0,
            1,
            1,
            second_bytes,
            cache.regions.len() as u64
        ]
    );
    let replacement = cache.insert(compiled(&memory, 0x1000));
    let replacement_bytes = replacement.wasm().len() as u64;
    assert_eq!(
        cache.snapshot()[..8],
        [
            3,
            first_bytes + second_bytes + replacement_bytes,
            0,
            0,
            1,
            2,
            second_bytes + replacement_bytes,
            cache.regions.len() as u64
        ]
    );
    bounds(&cache);
}

#[test]
fn owner_table_pointer_survives_cache_moves_unlink_and_source_removal() {
    let mut memory = memory();
    let mut cache = CoreCache32::default();
    let pointer = cache.link_owners_ptr();
    assert_eq!(pointer, cache.link_owners.as_ptr());
    assert_eq!(cache.link_owners.len(), cache.link_capacity() + 1);
    assert!(cache.link_owners.iter().all(|owner| *owner == 0));
    let old = cache.insert(compiled(&memory, 0x1000));
    cache.install_link(&old, 4096).unwrap();
    assert_eq!(cache.link_owners[4096], old.identity());
    let mut moved = cache;
    assert_eq!(moved.link_owners_ptr(), pointer);
    moved.unlink(4096, old.identity()).unwrap();
    assert_eq!(moved.link_owners[4096], 0);
    moved.install_link(&old, 4096).unwrap();
    memory.write(GuestAddress(0x1002), &[4]).unwrap();
    assert!(moved.lookup_linked(&memory, 0x1000).is_none());
    assert_eq!(moved.link_owners_ptr(), pointer);
    assert_eq!(moved.link_owners[0], 0);
    assert_eq!(moved.link_owners[4096], 0);
    bounds(&moved);
}

#[test]
fn frontier_contains_current_indexed_entries_even_without_installed_links() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    assert!(cache.frontier(&memory).is_empty());
    let first = cache.insert(compiled(&memory, 0x1000));
    let second = cache.insert(compiled(&memory, filler(0)));
    cache.install_link(&second, 7).unwrap();
    let snapshot = cache.snapshot();
    let mut frontier = cache.frontier(&memory);
    frontier.sort_unstable_by_key(|pc| pc.0);
    assert_eq!(
        frontier,
        [0x1000, 0x1005, 0x100a, 0x100d, filler(0)]
            .map(GuestAddress)
            .to_vec()
    );
    assert_eq!(cache.retained(&first).unwrap().slot, 0);
    assert_eq!(cache.oldest(), Some(first.key.index));
    assert_eq!(cache.newest(), Some(second.key.index));
    assert_eq!(cache.oldest_linked_slot(), 7);
    assert_eq!(cache.snapshot(), snapshot);
    assert_eq!(cache.frontier(&AddressSpace::new(1).unwrap()), []);
    assert_eq!(cache.snapshot(), snapshot);
    bounds(&cache);
}

#[test]
fn frontier_omits_stale_and_identically_remapped_sources_without_removing_owners() {
    for remap in [false, true] {
        let mut memory = memory();
        let mut cache = CoreCache32::default();
        let old = cache.insert(compiled(&memory, 0x1000));
        let current = cache.insert(compiled(&memory, filler(0)));
        cache.install_link(&old, 1).unwrap();
        cache.install_link(&current, 2).unwrap();
        let snapshot = cache.snapshot();
        if remap {
            let mut page = [0; 4096];
            memory.read(GuestAddress(0x1000), &mut page).unwrap();
            let range = PageRange::new(GuestAddress(0x1000), 1).unwrap();
            memory.unmap(range).unwrap();
            memory.map_zeroed(range, Permissions::ALL).unwrap();
            memory.write(GuestAddress(0x1000), &page).unwrap();
        } else {
            memory.write(GuestAddress(0x1002), &[4]).unwrap();
        }
        assert!(!old.is_current(&memory));
        assert_eq!(cache.frontier(&memory), vec![GuestAddress(filler(0))]);
        assert_eq!(cache.snapshot(), snapshot);
        assert!(cache.retained(&old).is_some());
        assert_eq!(cache.link_owners[1], old.identity());
        assert_eq!(cache.oldest(), Some(old.key.index));
        assert_eq!(cache.newest(), Some(current.key.index));
        bounds(&cache);
    }
}

#[test]
fn frontier_excludes_shadowed_side_entries_after_the_indexed_owner_is_removed() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let old = cache.insert(compiled(&memory, 0x1000));
    let newer = cache.insert(compiled(&memory, 0x1005));
    assert!(old.entrypoints().contains(&GuestAddress(0x1005)));
    assert!(newer.entrypoints().contains(&GuestAddress(0x100d)));
    cache.remove(newer.key);
    assert!(newer.is_current(&memory));
    assert!(cache.retained(&old).is_some());
    assert!(!cache.retained(&newer).is_some());
    let snapshot = cache.snapshot();
    let mut frontier = cache.frontier(&memory);
    frontier.sort_unstable_by_key(|pc| pc.0);
    assert_eq!(frontier, vec![GuestAddress(0x1000), GuestAddress(0x100a)]);
    assert_eq!(cache.snapshot(), snapshot);
    bounds(&cache);
}

#[test]
fn frontier_membership_accepts_linked_and_unlinked_entries_without_touching_lru() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let first = cache.insert(compiled(&memory, 0x1000));
    let second = cache.insert(compiled(&memory, filler(0)));
    cache.install_link(&first, 1).unwrap();
    let snapshot = cache.snapshot();
    let links = cache.link_owners.to_vec();
    for pc in [0x1000, 0x1005, 0x100a, 0x100d, filler(0)] {
        assert!(cache.frontier_contains(&memory, pc));
    }
    assert!(!cache.frontier_contains(&memory, 0x1001));
    assert!(!cache.frontier_contains(&memory, 0xdead_0000));
    assert_eq!(cache.retained(&second).unwrap().slot, 0);
    assert_eq!(cache.oldest(), Some(first.key.index));
    assert_eq!(cache.newest(), Some(second.key.index));
    assert_eq!(cache.oldest_linked_slot(), 1);
    assert_eq!(cache.snapshot(), snapshot);
    assert_eq!(cache.link_owners.as_slice(), links);
    bounds(&cache);
}

#[test]
fn frontier_membership_rejects_foreign_stale_and_remapped_sources_without_removal() {
    for remap in [false, true] {
        let mut memory = memory();
        let mut cache = CoreCache32::default();
        let first = cache.insert(compiled(&memory, 0x1000));
        let second = cache.insert(compiled(&memory, filler(0)));
        cache.install_link(&first, 1).unwrap();
        let snapshot = cache.snapshot();
        assert!(cache.frontier_contains(&memory, 0x1000));
        assert!(!cache.frontier_contains(&AddressSpace::new(1).unwrap(), 0x1000));
        if remap {
            let mut page = [0; 4096];
            memory.read(GuestAddress(0x1000), &mut page).unwrap();
            let range = PageRange::new(GuestAddress(0x1000), 1).unwrap();
            memory.unmap(range).unwrap();
            memory.map_zeroed(range, Permissions::ALL).unwrap();
            memory.write(GuestAddress(0x1000), &page).unwrap();
        } else {
            memory.write(GuestAddress(0x1002), &[4]).unwrap();
        }
        assert!(!cache.frontier_contains(&memory, 0x1000));
        assert!(cache.frontier_contains(&memory, filler(0)));
        assert_eq!(cache.snapshot(), snapshot);
        assert_eq!((cache.regions.len() - cache.free.len()), 2);
        assert_eq!(cache.link_owners[1], first.identity());
        assert_eq!(cache.oldest(), Some(first.key.index));
        assert_eq!(cache.newest(), Some(second.key.index));
        bounds(&cache);
    }
}

#[test]
fn frontier_membership_does_not_resurrect_a_removed_shadowing_owner() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let old = cache.insert(compiled(&memory, 0x1000));
    let newer = cache.insert(compiled(&memory, 0x1005));
    assert!(cache.frontier_contains(&memory, 0x1005));
    cache.remove(newer.key);
    assert!(newer.is_current(&memory));
    assert!(cache.retained(&old).is_some());
    assert!(!cache.frontier_contains(&memory, 0x1005));
    assert!(!cache.frontier_contains(&memory, 0x100d));
    assert!(cache.frontier_contains(&memory, 0x1000));
    assert!(cache.frontier_contains(&memory, 0x100a));
    bounds(&cache);
}

#[test]
fn reused_row_and_driver_slot_reject_a_held_current_owner() {
    let memory = memory();
    let source_version = memory.code_source_version();
    let mut cache = CoreCache32::default();
    let old = cache.insert(compiled(&memory, filler(0)));
    let neighbor = cache.insert(compiled(&memory, filler(1)));
    cache.install_link(&old, 1).unwrap();
    cache.install_link(&neighbor, 2).unwrap();
    cache.remove(old.key);
    let replacement = cache.insert(compiled(&memory, filler(2)));
    assert_eq!(replacement.key.index, old.key.index);
    assert_ne!(replacement.identity(), old.identity());
    cache.install_link(&replacement, 1).unwrap();
    assert!(old.is_current(&memory));
    assert_eq!(memory.code_source_version(), source_version);
    let snapshot = cache.snapshot();
    let owners = cache.link_owners.to_vec();
    let victim = cache.oldest_linked_slot();

    assert_eq!(cache.install_link(&old, 3), Err(LinkError::Unretained));
    assert_eq!(cache.unlink(1, old.identity()), Err(LinkError::WrongOwner));
    cache.remove(old.key);
    assert!(cache.lookup(&memory, filler(0)).is_none());
    assert!(!cache.frontier_contains(&memory, filler(0)));
    assert_eq!(cache.snapshot(), snapshot);
    assert_eq!(cache.link_owners.as_slice(), owners);
    assert_eq!(cache.oldest_linked_slot(), victim);
    assert_eq!(victim, 2);
    for (pc, expected, slot) in [(filler(1), &neighbor, 2), (filler(2), &replacement, 1)] {
        let (selected, actual_slot) = cache.lookup_linked(&memory, pc).unwrap();
        assert!(same(&selected, expected));
        assert_eq!(actual_slot, slot);
    }
    bounds(&cache);
}

#[test]
fn overlapping_entry_removal_and_row_reuse_preserve_exact_lookup_owners() {
    let memory = memory();
    let source_version = memory.code_source_version();
    let mut cache = CoreCache32::default();
    let old = cache.insert(compiled(&memory, 0x1000));
    let newer = cache.insert(compiled(&memory, 0x1005));
    cache.install_link(&old, 1).unwrap();
    cache.install_link(&newer, 2).unwrap();
    assert!(same(&cache.lookup(&memory, 0x1005).unwrap(), &newer));

    cache.remove(old.key);
    let first_replacement = cache.insert(compiled(&memory, filler(0)));
    assert_eq!(first_replacement.key.index, old.key.index);
    cache.install_link(&first_replacement, 1).unwrap();
    for pc in [0x1005, 0x100d] {
        let (selected, slot) = cache.lookup_linked(&memory, pc).unwrap();
        assert!(same(&selected, &newer));
        assert_eq!(slot, 2);
    }
    assert!(cache.lookup(&memory, 0x1000).is_none());
    assert!(cache.lookup(&memory, 0x100a).is_none());

    cache.remove(newer.key);
    let second_replacement = cache.insert(compiled(&memory, filler(1)));
    assert_eq!(second_replacement.key.index, newer.key.index);
    cache.install_link(&second_replacement, 2).unwrap();
    assert!(old.is_current(&memory) && newer.is_current(&memory));
    assert_eq!(memory.code_source_version(), source_version);
    let snapshot = cache.snapshot();
    let owners = cache.link_owners.to_vec();
    cache.remove(old.key);
    cache.remove(newer.key);
    for pc in [0x1000, 0x1001, 0x1005, 0x100a, 0x100d] {
        assert!(cache.lookup(&memory, pc).is_none());
        assert!(!cache.frontier_contains(&memory, pc));
    }
    assert_eq!(cache.snapshot(), snapshot);
    assert_eq!(cache.link_owners.as_slice(), owners);
    for (pc, expected, slot) in [
        (filler(0), &first_replacement, 1),
        (filler(1), &second_replacement, 2),
    ] {
        let (selected, actual_slot) = cache.lookup_linked(&memory, pc).unwrap();
        assert!(same(&selected, expected));
        assert_eq!(actual_slot, slot);
    }
    bounds(&cache);
}

#[test]
fn reused_link_endpoint_preserves_the_current_graph_and_rejects_old_slot_authority() {
    for replace_target in [false, true] {
        let mut memory = memory();
        for (pc, bytes) in [
            (0x4000, [0xff, 0xe6]),
            (0x4040, [0xff, 0xe7]),
            (0x4080, [0xff, 0xe6]),
        ] {
            memory.write(GuestAddress(pc), &bytes).unwrap();
        }
        let mut cache = CoreCache32::default();
        let source = cache.insert(compiled(&memory, 0x4000));
        let target = cache.insert(compiled(&memory, 0x4040));
        cache.install_link(&source, 11).unwrap();
        cache.install_link(&target, 12).unwrap();
        assert!(matches!(cache.lookup_transition(&memory,&source,0x4040),
            TransitionLookup::Linked(ref code,12) if same(code,&target)));
        let (removed, slot) = if replace_target {
            (&target, 12)
        } else {
            (&source, 11)
        };
        cache.remove(removed.key);
        let replacement = cache.insert(compiled(&memory, 0x4080));
        assert_eq!(replacement.key.index, removed.key.index);
        cache.install_link(&replacement, slot).unwrap();
        let snapshot = cache.snapshot();
        let owners = cache.link_owners.to_vec();
        assert_eq!(cache.install_link(removed, 40), Err(LinkError::Unretained));
        assert_eq!(
            cache.unlink(slot, removed.identity()),
            Err(LinkError::WrongOwner)
        );
        let (current_source, current_target, pc, target_slot) = if replace_target {
            (&source, &replacement, 0x4080, 12)
        } else {
            (&replacement, &target, 0x4040, 12)
        };
        for _ in 0..8193 {
            assert!(matches!(cache.lookup_transition(&memory,current_source,pc),
                TransitionLookup::Linked(ref code,selected) if same(code,current_target)&&selected==target_slot));
        }
        assert_eq!(cache.snapshot(), snapshot);
        assert_eq!(cache.link_owners.as_slice(), owners);
        bounds(&cache);
    }
}

#[test]
fn transition_lookup_caches_four_targets_and_self_edges_without_changing_owners() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let owners: Vec<_> = (0..5)
        .map(|index| {
            let code = cache.insert(compiled(&memory, filler(index)));
            cache.install_link(&code, index as u32 + 1).unwrap();
            code
        })
        .collect();
    let source = &owners[0];
    for _ in 0..2 {
        for (index, target) in owners.iter().take(4).enumerate() {
            let pc = filler(index);
            assert!(matches!(
                cache.lookup_transition(&memory, source, pc),
                TransitionLookup::Linked(ref code, slot)
                    if same(code, target) && slot == index as u32 + 1
            ));
            assert_eq!(
                cache
                    .retained(source)
                    .unwrap()
                    .linked_entries
                    .get(cache.entries_epoch, pc),
                Some(target.key)
            );
            assert_eq!(cache.newest(), Some(target.key.index));
        }
    }
    assert!(matches!(
        cache.lookup_transition(&memory, source, filler(4)),
        TransitionLookup::Linked(ref code, 5) if same(code, &owners[4])
    ));
    let linked = &cache.retained(source).unwrap().linked_entries;
    assert_eq!(linked.get(cache.entries_epoch, filler(0)), None);
    assert_eq!(
        linked.get(cache.entries_epoch, filler(4)),
        Some(owners[4].key)
    );
    bounds(&cache);
}

#[test]
fn cached_transition_lookup_follows_partial_entry_shadowing() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let source = cache.insert(compiled(&memory, filler(0)));
    let target = cache.insert(compiled(&memory, 0x1000));
    cache.install_link(&source, 1).unwrap();
    cache.install_link(&target, 2).unwrap();
    assert!(matches!(
        cache.lookup_transition(&memory, &source, 0x1005),
        TransitionLookup::Linked(ref code, 2) if same(code, &target)
    ));
    let epoch = cache.entries_epoch;
    let shadow = cache.insert(compiled(&memory, 0x1005));
    cache.install_link(&shadow, 3).unwrap();
    assert!(cache.retained(&target).is_some());
    assert_ne!(cache.entries_epoch, epoch);
    assert!(matches!(
        cache.lookup_transition(&memory, &source, 0x1005),
        TransitionLookup::Linked(ref code, 3) if same(code, &shadow)
    ));
    assert!(same(&cache.lookup(&memory, 0x1000).unwrap(), &target));
    assert!(matches!(
        cache.lookup_transition(&memory, &source, 0x1000),
        TransitionLookup::Linked(ref code, 2) if same(code, &target)
    ));
    cache.remove(target.key);
    let replacement = cache.insert(compiled(&memory, filler(1)));
    assert_eq!(replacement.key.index, target.key.index);
    cache.install_link(&replacement, 2).unwrap();
    assert!(target.is_current(&memory));
    assert!(matches!(
        cache.lookup_transition(&memory, &source, 0x1000),
        TransitionLookup::Miss
    ));
    assert!(matches!(
        cache.lookup_transition(&memory, &source, filler(1)),
        TransitionLookup::Linked(ref code, 2) if same(code, &replacement)
    ));

    bounds(&cache);
}

#[test]
fn cached_transition_lookup_rechecks_live_links_without_an_index_change() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let source = cache.insert(compiled(&memory, filler(0)));
    let target = cache.insert(compiled(&memory, 0x1000));
    let other = cache.insert(compiled(&memory, filler(1)));
    cache.install_link(&source, 1).unwrap();
    cache.install_link(&target, 2).unwrap();
    assert!(matches!(
        cache.lookup_transition(&memory, &source, 0x1000),
        TransitionLookup::Linked(ref code, 2) if same(code, &target)
    ));
    let epoch = cache.entries_epoch;
    cache.unlink(2, target.identity()).unwrap();
    cache.install_link(&other, 2).unwrap();
    assert_eq!(cache.entries_epoch, epoch);
    assert!(matches!(
        cache.lookup_transition(&memory, &source, 0x1000),
        TransitionLookup::Miss
    ));
    assert_eq!(cache.newest(), Some(target.key.index));
    cache.install_link(&target, 4).unwrap();
    assert!(matches!(
        cache.lookup_transition(&memory, &source, 0x1000),
        TransitionLookup::Linked(ref code, 4) if same(code, &target)
    ));
    assert_eq!(cache.link_owners[2], other.identity());
    assert_eq!(cache.entries_epoch, epoch);

    bounds(&cache);
}

#[test]
fn entry_epoch_wrap_invalidates_every_retained_sources_cached_authority() {
    for insert in [false, true] {
        let memory = memory();
        let mut cache = CoreCache32::default();
        let source = cache.insert(compiled(&memory, filler(0)));
        let target = cache.insert(compiled(&memory, 0x1000));
        let other = cache.insert(compiled(&memory, filler(1)));
        let victim = cache.insert(compiled(&memory, filler(2)));
        cache.install_link(&source, 1).unwrap();
        cache.install_link(&target, 2).unwrap();
        cache.install_link(&other, 3).unwrap();
        cache.entries_epoch = u64::MAX;
        assert!(matches!(
            cache.lookup_transition(&memory, &source, 0x1000),
            TransitionLookup::Linked(ref code, 2) if same(code, &target)
        ));
        cache.regions[other.key.index]
            .as_mut()
            .unwrap()
            .linked_entries
            .remember(1, 0x1000, source.key);
        if insert {
            cache.insert(compiled(&memory, filler(3)));
        } else {
            cache.remove(victim.key);
        }
        assert_eq!(cache.entries_epoch, 1);
        assert!(
            cache
                .regions
                .iter()
                .flatten()
                .all(|entry| entry.linked_entries.epoch == 0)
        );
        assert!(matches!(
            cache.lookup_transition(&memory, &other, 0x1000),
            TransitionLookup::Linked(ref code, 2) if same(code, &target)
        ));
        bounds(&cache);
    }
}

#[test]
fn repeated_compiled_edges_keep_immutable_owners_and_never_request_reformation() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let source = cache.insert(compiled(&memory, filler(0)));
    let target = cache.insert(compiled(&memory, 0x1000));
    cache.install_link(&source, 1).unwrap();
    cache.install_link(&target, 2).unwrap();
    let snapshot = cache.snapshot();
    let source_bytes = source.wasm().to_vec();
    let target_bytes = target.wasm().to_vec();
    for _ in 0..8193 {
        for (from, pc, to, slot) in [
            (&source, 0x1000, &target, 2),
            (&target, filler(0), &source, 1),
        ] {
            assert!(matches!(cache.lookup_transition(&memory,from,pc),
                TransitionLookup::Linked(ref code,selected) if same(code,to)&&selected==slot));
            assert_eq!(cache.newest(), Some(to.key.index));
        }
    }
    assert_eq!(cache.snapshot(), snapshot);
    assert_eq!(source.wasm(), source_bytes);
    assert_eq!(target.wasm(), target_bytes);
    assert_eq!(
        &cache.link_owners[1..3],
        &[source.identity(), target.identity()]
    );
    bounds(&cache);
}

#[test]
fn transition_lookup_keeps_unlinked_lookup_recency_without_replacing_owners() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let source = cache.insert(compiled(&memory, filler(0)));
    let target = cache.insert(compiled(&memory, 0x1000));
    let last = cache.insert(compiled(&memory, filler(1)));
    cache.install_link(&source, 1).unwrap();
    cache.install_link(&last, 3).unwrap();
    assert_eq!(cache.newest(), Some(last.key.index));
    assert!(matches!(
        cache.lookup_transition(&memory, &source, 0x1000),
        TransitionLookup::Miss
    ));
    assert_eq!(cache.newest(), Some(target.key.index));

    cache.install_link(&target, 2).unwrap();
    assert!(matches!(
        cache.lookup_transition(&memory, &source, 0x1000),
        TransitionLookup::Linked(_, 2)
    ));

    bounds(&cache);
}

#[test]
fn transition_lookup_rejects_stale_and_identically_remapped_targets() {
    for remap in [false, true] {
        let mut memory = memory();
        let mut cache = CoreCache32::default();
        let source = cache.insert(compiled(&memory, filler(0)));
        let target = cache.insert(compiled(&memory, 0x1000));
        cache.install_link(&source, 1).unwrap();
        cache.install_link(&target, 2).unwrap();
        assert!(matches!(
            cache.lookup_transition(&memory, &source, 0x1000),
            TransitionLookup::Linked(ref code, 2) if same(code, &target)
        ));
        if remap {
            let mut bytes = [0; 4096];
            memory.read(GuestAddress(0x1000), &mut bytes).unwrap();
            let range = PageRange::new(GuestAddress(0x1000), 1).unwrap();
            memory.unmap(range).unwrap();
            memory.map_zeroed(range, Permissions::ALL).unwrap();
            memory.write(GuestAddress(0x1000), &bytes).unwrap();
        } else {
            memory.write(GuestAddress(0x1002), &[4]).unwrap();
        }
        assert!(source.is_current(&memory));
        assert!(matches!(
            cache.lookup_transition(&memory, &source, 0x1000),
            TransitionLookup::Miss
        ));
        assert_eq!(cache.source_invalidations, 1);
        assert_eq!(cache.link_owners[2], 0);
        assert!(cache.retained(&target).is_none());

        bounds(&cache);
    }
}

#[test]
fn transition_lookup_never_uses_held_source_ownership_after_row_and_slot_reuse() {
    let memory = memory();
    let mut cache = CoreCache32::default();
    let old = cache.insert(compiled(&memory, filler(0)));
    let target = cache.insert(compiled(&memory, 0x1000));
    cache.install_link(&old, 1).unwrap();
    cache.install_link(&target, 2).unwrap();
    assert!(matches!(
        cache.lookup_transition(&memory, &old, 0x1000),
        TransitionLookup::Linked(ref code, 2) if same(code, &target)
    ));
    cache.remove(old.key);
    let replacement = cache.insert(compiled(&memory, filler(1)));
    assert_eq!(replacement.key.index, old.key.index);
    cache.install_link(&replacement, 1).unwrap();
    assert!(old.is_current(&memory));
    let snapshot = cache.snapshot();
    match cache.lookup_transition(&memory, &old, 0x1000) {
        TransitionLookup::Linked(code, slot) => {
            assert!(same(&code, &target));
            assert_eq!(slot, 2);
        }
        _ => panic!("a valid target remains dispatchable without a retained source"),
    }

    assert_eq!(cache.snapshot(), snapshot);
    assert_eq!(cache.link_owners[1], replacement.identity());
    assert!(matches!(
        cache.lookup_transition(&memory, &replacement, 0x1000),
        TransitionLookup::Linked(_, 2)
    ));

    bounds(&cache);
}

#[test]
fn transition_lookup_selects_a_shadowing_target_and_stops_before_a_stale_source() {
    let mut memory = memory();
    let mut cache = CoreCache32::default();
    let source = cache.insert(compiled(&memory, filler(0)));
    let target = cache.insert(compiled(&memory, 0x1000));
    cache.install_link(&source, 1).unwrap();
    cache.install_link(&target, 2).unwrap();
    let shadow = cache.insert(compiled(&memory, 0x1000));
    cache.install_link(&shadow, 3).unwrap();
    match cache.lookup_transition(&memory, &source, 0x1000) {
        TransitionLookup::Linked(code, slot) => {
            assert!(same(&code, &shadow));
            assert_eq!(slot, 3);
        }
        _ => panic!("the current index must select the shadowing owner"),
    }
    assert!(matches!(
        cache.lookup_transition(&memory, &source, 0x1000),
        TransitionLookup::Linked(ref code, 3) if same(code, &shadow)
    ));
    memory.write(GuestAddress(filler(0)), &[0x40]).unwrap();
    let snapshot = cache.snapshot();
    let newest = cache.newest();
    assert!(matches!(
        cache.lookup_transition(&memory, &source, 0x1000),
        TransitionLookup::Stop
    ));

    assert_eq!(cache.snapshot(), snapshot);
    assert_eq!(cache.newest(), newest);
    bounds(&cache);
}
