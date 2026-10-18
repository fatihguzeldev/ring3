use super::super::slots::FreeSlots;

#[test]
fn free_slots_preserve_lowest_slot_across_word_boundaries_and_reuse() {
    for count in [1, 63, 64, 65, 4096, 65_536] {
        let mut free = FreeSlots::new(count).unwrap();
        for slot in 0..count as u32 {
            assert_eq!(free.first(), Some(slot));
            free.occupy(slot);
        }
        assert_eq!(free.first(), None);
        for slot in [0, count as u32 - 1, count as u32 / 2] {
            free.release(slot);
        }
        assert_eq!(free.first(), Some(0));
        free.occupy(0);
        if count > 1 {
            assert_eq!(free.first(), Some(count as u32 / 2));
            free.occupy(count as u32 / 2);
            if count > 2 {
                assert_eq!(free.first(), Some(count as u32 - 1));
            }
        }
    }
}
