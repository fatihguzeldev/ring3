use super::*;

fn page(number: u32, offsets: &[u16], ids: &[UnitId]) -> EntryPage {
    assert_eq!(offsets.len(), ids.len());
    EntryPage {
        page: number,
        offsets: offsets.to_vec(),
        units: ids.to_vec(),
    }
}

#[test]
fn page_positions_recheck_after_insertion_retirement_last_removal_and_reuse() {
    let old = UnitId(0x1234_5678_0000_0001);
    let new = UnitId(0x1234_5678_0000_0002);
    let mut index = EntryIndex {
        pages: vec![
            page(1, &[0], &[old]),
            page(3, &[0], &[old]),
            page(5, &[0], &[old]),
        ],
        ..EntryIndex::default()
    };
    assert_eq!(index.candidates(3 * PAGE_SIZE), &[old]);
    assert_eq!(index.page_hints[3].load(Ordering::Relaxed), 1);
    index.pages.insert(0, page(0, &[0], &[new]));
    assert_eq!(index.candidates(3 * PAGE_SIZE), &[old]);
    assert_eq!(index.page_hints[3].load(Ordering::Relaxed), 2);
    index.pages.remove(0);
    assert_eq!(index.candidates(3 * PAGE_SIZE), &[old]);
    assert_eq!(index.page_hints[3].load(Ordering::Relaxed), 1);
    assert_eq!(index.candidates(5 * PAGE_SIZE), &[old]);
    index.pages.remove(1);
    assert!(index.candidates(3 * PAGE_SIZE).is_empty());
    assert_eq!(index.candidates(5 * PAGE_SIZE), &[old]);
    assert_eq!(index.page_hints[5].load(Ordering::Relaxed), 1);
    index.pages.insert(1, page(3, &[0], &[new]));
    assert_eq!(index.candidates(3 * PAGE_SIZE), &[new]);
    assert_eq!(index.candidates(5 * PAGE_SIZE), &[old]);
    index.pages.pop();
    assert!(index.candidates(5 * PAGE_SIZE).is_empty());
    index.pages.push(page(5, &[0], &[new]));
    assert_eq!(index.candidates(5 * PAGE_SIZE), &[new]);
}

#[test]
fn colliding_page_hints_preserve_duplicate_full_ids_holes_and_unsigned_endpoints() {
    let ids = [
        UnitId(0x0000_0001_0000_0001),
        UnitId(0x0000_0002_0000_0001),
        UnitId(0x0000_0003_0000_0001),
        UnitId(u64::MAX - 1),
    ];
    let index = EntryIndex {
        pages: [0, 64, 128, 0xfffff]
            .map(|number| page(number, &[0, 2, 2, 4095], &ids))
            .into_iter()
            .collect(),
        ..EntryIndex::default()
    };
    for number in [128, 0, 64, 128, 64, 0, 0xfffff] {
        let pc = number * PAGE_SIZE;
        assert_eq!(index.candidates(pc), &ids[..1]);
        assert!(index.candidates(pc + 1).is_empty());
        assert_eq!(index.candidates(pc + 2), &ids[1..3]);
        assert!(index.candidates(pc + 3).is_empty());
        assert_eq!(index.candidates(pc + 4095), &ids[3..]);
    }
    for position in [usize::MAX, 100, 1] {
        index.page_hints[0].store(position, Ordering::Relaxed);
        assert_eq!(index.candidates(0), &ids[..1]);
    }
    assert!(index.candidates(PAGE_SIZE).is_empty());
}

#[test]
fn hints_never_cache_candidate_membership_and_retain_sync() {
    fn assert_sync<T: Sync>() {}
    assert_sync::<EntryIndex>();
    let first = UnitId(0x0000_0001_0000_0001);
    let second = UnitId(0x0000_0002_0000_0001);
    let mut index = EntryIndex::default();
    assert!(index.candidates(0).is_empty());
    index.page_hints[0].store(0, Ordering::Relaxed);
    assert!(index.candidates(0).is_empty());
    index.pages.push(page(64, &[0], &[first]));
    assert_eq!(index.candidates(64 * PAGE_SIZE), &[first]);
    index.pages[0].offsets.push(0);
    index.pages[0].units.push(second);
    assert_eq!(index.candidates(64 * PAGE_SIZE), &[first, second]);
    index.remove(first, &[64]);
    assert_eq!(index.candidates(64 * PAGE_SIZE), &[second]);
    index.remove(second, &[64]);
    assert!(index.candidates(64 * PAGE_SIZE).is_empty());
    index.pages.push(page(64, &[0], &[second]));
    assert_eq!(index.candidates(64 * PAGE_SIZE), &[second]);
}
