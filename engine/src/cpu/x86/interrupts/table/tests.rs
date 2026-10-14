use super::*;

#[test]
fn raw_slots_and_limits_do_not_imply_gate_validity_or_inline_engine_storage() {
    let records = Box::new(std::array::from_fn(|vector| {
        SegmentDescriptorRecord::from_bytes(std::array::from_fn(|byte| {
            (vector * 8 + byte).wrapping_mul(37).wrapping_add(0xa5) as u8
        }))
    }));
    let table = InterruptDescriptorTable32::new(0, records);
    assert_eq!(table.limit(), 0);
    for vector in 0..=u8::MAX {
        assert_eq!(
            table.record(vector).to_bytes(),
            std::array::from_fn(|byte| {
                (usize::from(vector) * 8 + byte)
                    .wrapping_mul(37)
                    .wrapping_add(0xa5) as u8
            })
        );
    }
    assert!(std::mem::size_of::<Option<InterruptDescriptorTable32>>() < 64);
    let table = InterruptDescriptorTable32::new(
        u16::MAX,
        Box::new([SegmentDescriptorRecord::from_words(0, 0); 256]),
    );
    assert_eq!(table.limit(), u16::MAX);
    assert_eq!(table.record(0).to_words(), (0, 0));
    assert_eq!(table.record(255).to_words(), (0, 0));
}
