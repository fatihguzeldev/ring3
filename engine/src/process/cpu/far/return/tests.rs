use super::super::support::{engine, field, only_packet_changed};
use super::*;
use crate::cpu::x86::SegmentDescriptorRecord;

#[test]
fn return_requires_rpl_three_even_for_conforming_code_without_partial_cache_publication() {
    let mut engine = engine();
    engine
        .configure_segment_descriptor_raw(
            0x43,
            SegmentDescriptorRecord::from_words(0xffff, 0x00cf9c00),
        )
        .unwrap();
    for selector in [0x40, 0x41, 0x42] {
        let before = engine.arena().to_vec();
        let table = engine.segment_descriptors.clone();
        engine.far_return(0x2000, selector, 0).unwrap();
        assert_eq!((field(&engine, 16), field(&engine, 24)), (3, 0x40));
        only_packet_changed(&before, &engine);
        assert_eq!(engine.segment_descriptors, table);
    }
    engine.far_return(0xffff2345, 0x43, 1).unwrap();
    assert_eq!(field(&engine, 20), 0x2345);
    assert_eq!(engine.segment_context().unwrap().state.selectors[1], 0x43);
    assert_eq!(
        engine
            .segment_descriptors
            .raw_descriptor(0x43)
            .unwrap()
            .descriptor_type(),
        13
    );
}

#[test]
fn return_profile_and_stale_authority_refusals_leave_full_state_untouched() {
    let mut engine = engine();
    engine
        .configure_segment_descriptor_raw(
            0x43,
            SegmentDescriptorRecord::from_words(0xffff, 0x00affa00),
        )
        .unwrap();
    let before = engine.arena().to_vec();
    let table = engine.segment_descriptors.clone();
    assert_eq!(
        engine.far_return(1, 0x43, 0),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.segment_descriptors, table);
    assert!(
        engine
            .far_return_resident(engine.key ^ 1, 1, 1, 0x43, 0)
            .is_err()
    );
    assert_eq!(engine.arena(), before);
}
