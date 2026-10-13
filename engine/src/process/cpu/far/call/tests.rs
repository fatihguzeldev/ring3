use super::super::support::{engine, field, only_packet_changed};
use super::*;
use crate::{
    abi::{arena::SEGMENTS_OFFSET, x86::encode_segment_context},
    cpu::x86::{SegmentCache, SegmentDescriptorRecord},
    memory::GuestAddress,
};

#[test]
fn call_commits_two_exact_slots_from_live_esp_with_width_specific_return_ip() {
    for word in [0, 1] {
        let mut engine = engine();
        let slot = if word == 1 { 2 } else { 4 };
        let before = engine.arena().to_vec();
        let table = engine.segment_descriptors.clone();
        assert_eq!(
            engine.far_call(0xffff2345, 0x40, word, 0xabcd1234, 0x7800),
            Ok(StoreCompletion::Complete)
        );
        let mut bytes = [0; 8];
        engine
            .memory()
            .unwrap()
            .read(
                GuestAddress(0x7800 - 2 * slot),
                &mut bytes[..2 * slot as usize],
            )
            .unwrap();
        let ip = if word == 1 { 0x1234_u32 } else { 0xabcd1234 };
        assert_eq!(&bytes[..slot as usize], &ip.to_le_bytes()[..slot as usize]);
        assert_eq!(
            &bytes[slot as usize..2 * slot as usize],
            &0x1b_u32.to_le_bytes()[..slot as usize]
        );
        assert_eq!(field(&engine, 16), 0);
        assert_eq!(
            field(&engine, 20),
            if word == 1 { 0x2345 } else { 0xffff2345 }
        );
        assert_eq!(engine.segment_context().unwrap().state.selectors[1], 0x43);
        assert_ne!(engine.segment_descriptors, table);
        let mut expected = before;
        for (offset, length) in [
            (HELPER_OFFSET, HELPER_SIZE),
            (SEGMENTS_OFFSET, SEGMENTS_SIZE),
        ] {
            expected[offset..offset + length]
                .copy_from_slice(&engine.arena()[offset..offset + length]);
        }
        assert_eq!(
            engine.arena(),
            expected,
            "helper does not publish live registers or EIP"
        );
    }
}

#[test]
fn descriptor_presence_precedes_stack_capacity_which_precedes_target_limit() {
    let mut engine = engine();
    let mut context = engine.segment_context().unwrap();
    context.caches[2] = SegmentCache::from_bits(0xf23077ff).unwrap();
    let mut bytes = [0; SEGMENTS_SIZE];
    encode_segment_context(&context, &mut bytes).unwrap();
    engine.arena_mut().unwrap()[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE]
        .copy_from_slice(&bytes);
    for (high, expected) in [
        (0x0040f800, (7, 0)),
        (0x00407800, (6, 0x40)),
        (0x0040d800, (3, 0x40)),
    ] {
        engine
            .configure_segment_descriptor_raw(
                0x43,
                SegmentDescriptorRecord::from_words(0xffff, high),
            )
            .unwrap();
        let before = engine.arena().to_vec();
        let table = engine.segment_descriptors.clone();
        engine.far_call(0x10000, 0x43, 0, 0x1007, 0x7808).unwrap();
        assert_eq!((field(&engine, 16), field(&engine, 24)), expected);
        only_packet_changed(&before, &engine);
        assert_eq!(engine.segment_descriptors, table);
    }
}

#[test]
fn second_stack_page_and_version_failure_leave_ram_cs_and_descriptor_unchanged() {
    for exhausted in [false, true] {
        let mut engine = engine();
        if exhausted {
            engine.memory.as_mut().unwrap().exhaust_versions_for_test();
        }
        let esp = if exhausted { 0x7800 } else { 0x7004 };
        let before = engine.arena().to_vec();
        let table = engine.segment_descriptors.clone();
        let mut ram = [0; 8];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(0x7000), &mut ram)
            .unwrap();
        let version = engine.memory().unwrap().code_validation_version();
        engine.far_call(0x2000, 0x43, 0, 0x1007, esp).unwrap();
        assert_eq!(
            (field(&engine, 16), field(&engine, 24)),
            if exhausted { (2, 1) } else { (1, 1) }
        );
        only_packet_changed(&before, &engine);
        assert_eq!(engine.segment_descriptors, table);
        let mut after = [0; 8];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(0x7000), &mut after)
            .unwrap();
        assert_eq!(after, ram);
        assert_eq!(engine.memory().unwrap().code_validation_version(), version);
    }
}

#[test]
fn invalid_call_authority_and_malformed_requests_are_neutral() {
    let mut engine = engine();
    for (selector, word) in [(0x10043, 0), (0x43, 2)] {
        let before = engine.arena().to_vec();
        let table = engine.segment_descriptors.clone();
        assert_eq!(
            engine.far_call(0x2000, selector, word, 0x1007, 0x7800),
            Err(HostError::InvalidRequest)
        );
        assert_eq!(engine.arena(), before);
        assert_eq!(engine.segment_descriptors, table);
    }
    let before = engine.arena().to_vec();
    assert!(
        engine
            .far_call_resident(engine.key ^ 1, 1, 0x2000, 0x43, 0, 0x1007, 0x7800)
            .is_err()
    );
    assert_eq!(engine.arena(), before);
}
