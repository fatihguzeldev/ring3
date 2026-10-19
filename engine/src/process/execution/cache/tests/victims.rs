use super::super::{context::CacheEntry32, victims::VictimIndex};
use crate::cpu::dbt::CompilationTier32;
use std::{
    cell::Cell,
    collections::BTreeSet,
    sync::atomic::{AtomicU64, Ordering},
};

fn row(id: u64, last: u64) -> CacheEntry32 {
    CacheEntry32 {
        id,
        entry: 0,
        bytes: 0,
        tier: CompilationTier32::Baseline,
        hits: AtomicU64::new(0),
        last: AtomicU64::new(last),
        promotion_retry: Default::default(),
    }
}

#[test]
fn indexed_victims_match_stale_first_lru_scan_across_touches_pins_versions_and_slot_reuse() {
    let mut index = VictimIndex::new(65).unwrap();
    let mut entries = (0..65).map(|_| None).collect::<Vec<_>>();
    let mut stale = BTreeSet::new();
    let mut version = 0;
    let mut identity = 0;
    let mut clock = 0;
    let mut random = 71u64;
    for step in 0..2000 {
        random = random.wrapping_mul(6364136223846793005).wrapping_add(1);
        let slot = (random >> 32) as usize % entries.len();
        clock += 1;
        match step % 4 {
            0 => {
                identity += 1;
                index.install(slot as u32, identity, clock);
                entries[slot] = Some(row(identity, clock));
            }
            1 => {
                if let Some(row) = &entries[slot] {
                    row.last.store(clock, Ordering::Relaxed);
                }
            }
            2 => {
                index.remove(slot as u32);
                entries[slot] = None;
            }
            _ => {
                if let Some(row) = &entries[slot] {
                    stale.insert(row.id);
                }
                version += 1;
            }
        }
        let mut pins = entries
            .iter()
            .enumerate()
            .filter(|(slot, _)| (slot + step) % 5 == 0)
            .filter_map(|(_, row)| row.as_ref().map(|row| row.id))
            .collect::<Vec<_>>();
        pins.sort_unstable();
        let expected = entries
            .iter()
            .enumerate()
            .filter_map(|(slot, row)| row.as_ref().map(|row| (slot, row)))
            .filter(|(_, row)| pins.binary_search(&row.id).is_err())
            .min_by_key(|(slot, row)| {
                (
                    !stale.contains(&row.id),
                    row.last.load(Ordering::Relaxed),
                    *slot,
                )
            })
            .map(|(slot, _)| slot as u32);
        let selected = index.select(&entries, version, |id| !stale.contains(&id), &pins);
        assert_eq!(selected, expected, "step {step}");
        assert_eq!(index.len(), entries.iter().flatten().count());
        let checks = Cell::new(0);
        assert_eq!(
            index.select(
                &entries,
                version,
                |id| {
                    checks.set(checks.get() + 1);
                    !stale.contains(&id)
                },
                &pins
            ),
            expected
        );
        assert_eq!(
            checks.get(),
            0,
            "unchanged code version does not rescan validity"
        );
    }
}

#[test]
fn all_pinned_selection_preserves_every_candidate_for_later_unpinning() {
    let entries = (0..64)
        .map(|slot| Some(row(slot + 1, slot / 2)))
        .collect::<Vec<_>>();
    let mut index = VictimIndex::new(entries.len()).unwrap();
    for (slot, row) in entries.iter().enumerate() {
        let row = row.as_ref().unwrap();
        index.install(slot as u32, row.id, row.last.load(Ordering::Relaxed));
    }
    let pins = (1..=64).collect::<Vec<_>>();
    for _ in 0..3 {
        assert_eq!(index.select(&entries, 0, |_| true, &pins), None);
        assert_eq!(index.len(), 64);
        assert_eq!(index.select(&entries, 0, |_| true, &[]), Some(0));
    }
    assert_eq!(index.select(&entries, 1, |id| id != 64, &[]), Some(63));
    assert_eq!(
        index.select(&entries, 1, |_| panic!("cached version"), &[64]),
        Some(0)
    );
}
