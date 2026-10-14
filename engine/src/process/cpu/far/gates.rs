use super::support::{engine, field, only_packet_changed};
use super::*;
use crate::{
    abi::{arena::SEGMENTS_OFFSET, x86::encode_segment_context},
    cpu::x86::{SegmentCache, SegmentDescriptorRecord},
    memory::GuestAddress,
    process::StoreCompletion,
};

#[test]
fn gate_stack_limit_target_limit_and_second_page_fault_preserve_table_before_final_commit() {
    let mut engine = engine();
    let gate = |offset: u32| {
        SegmentDescriptorRecord::from_words(
            0x004b0000 | (offset & 0xffff),
            (offset & 0xffff0000) | 0xec00,
        )
    };
    let target = SegmentDescriptorRecord::from_words(0xffff, 0x0040fa00);
    engine
        .configure_segment_descriptor_raw(0x4b, target)
        .unwrap();
    engine
        .configure_segment_descriptor_raw(0x43, gate(0x10000))
        .unwrap();
    let original = engine.segment_context().unwrap();
    let mut bounded = original;
    bounded.caches[2] = SegmentCache::from_bits(0xf23077ff).unwrap();
    let mut record = [0; SEGMENTS_SIZE];
    encode_segment_context(&bounded, &mut record).unwrap();
    engine.arena_mut().unwrap()[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE]
        .copy_from_slice(&record);
    let mut ram = [0; 4096];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x7000), &mut ram)
        .unwrap();
    for (esp, expected) in [(0x7808, (7, 0)), (0x7800, (3, 0)), (0x7004, (9, 6))] {
        if esp == 0x7800 {
            encode_segment_context(&original, &mut record).unwrap();
            engine.arena_mut().unwrap()[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE]
                .copy_from_slice(&record);
        }
        if esp == 0x7004 {
            engine
                .configure_segment_descriptor_raw(0x43, gate(0x2000))
                .unwrap();
        }
        let before = engine.arena().to_vec();
        let table = engine.segment_descriptors.clone();
        assert_eq!(
            engine.far_call(0xdeadbeef, 0x43, 1, 0x1006, esp),
            Ok(StoreCompletion::Complete)
        );
        assert_eq!((field(&engine, 16), field(&engine, 24)), expected);
        if esp == 0x7004 {
            super::support::page_fault_packet(&engine, [9, 0, 6, 0x6ffc, 2, 4]);
        } else {
            assert_eq!((field(&engine, 4), field(&engine, 36)), (0x10009, 4));
            assert_eq!(engine.cr2(), None);
        }
        only_packet_changed(&before, &engine);
        assert_eq!(engine.segment_descriptors, table);
        let mut after = [0; 4096];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(0x7000), &mut after)
            .unwrap();
        assert_eq!(after, ram);
    }
    let before_gate = *engine.segment_descriptors.raw_descriptor(0x43).unwrap();
    assert_eq!(
        engine.far_call(0xdeadbeef, 0x43, 1, 0x1006, 0x7800),
        Ok(StoreCompletion::Complete)
    );
    assert_eq!(field(&engine, 16), 0);
    assert_eq!(
        *engine.segment_descriptors.raw_descriptor(0x43).unwrap(),
        before_gate
    );
    let (low, high) = target.to_words();
    assert_eq!(
        *engine.segment_descriptors.raw_descriptor(0x4b).unwrap(),
        SegmentDescriptorRecord::from_words(low, high | 0x100)
    );
}
