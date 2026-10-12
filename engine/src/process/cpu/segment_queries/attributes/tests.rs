use super::*;
use crate::{
    abi::{
        arena::{HELPER_OFFSET, SEGMENTS_OFFSET, TRANSFER_OFFSET},
        cpu_helper::encode_cpu_result,
    },
    cpu::x86::SegmentDescriptorRecord,
};

fn engine() -> EngineInstance {
    let mut engine = EngineInstance::new(4, 0x4c4152).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
        .copy_from_slice(&[0xeb, 0xfe]);
    engine.upload(0x1000, 2).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0, 0x10, 0, 0, 2, 0, 0, 0]);
    engine.compile(1).unwrap();
    engine
}

fn input(engine: &mut EngineInstance, selector: u32, flags: u32) -> [u32; 6] {
    let words = [
        selector,
        flags,
        0xaabb_ccdd,
        0x1122_3344,
        0x5566_7788,
        0x99aa_bbcc,
    ];
    for (index, value) in words.into_iter().enumerate() {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&value.to_le_bytes());
    }
    words
}

#[test]
fn raw_queries_publish_exact_value_and_zf_only_without_accessed_or_cache_mutation() {
    let mut engine = engine();
    let raw = SegmentDescriptorRecord::from_words(0x1234_2345, 0xabd1_7278);
    engine.configure_segment_descriptor_raw(0x43, raw).unwrap();
    for (kind, value) in [(0x30003, 0x00d0_7200_u32), (0x30004, 0x1234_5fff)] {
        for flags in [2, 0x24_4ed7] {
            let old = input(&mut engine, 0x43, flags);
            let mut expected = engine.arena().to_vec();
            expected[TRANSFER_OFFSET..TRANSFER_OFFSET + 4].copy_from_slice(&value.to_le_bytes());
            expected[TRANSFER_OFFSET + 4..TRANSFER_OFFSET + 8]
                .copy_from_slice(&(old[1] | 0x40).to_le_bytes());
            encode_cpu_result(
                0,
                0,
                Ok(()),
                &mut expected[HELPER_OFFSET..HELPER_OFFSET + 40],
            )
            .unwrap();
            let table = engine.segment_descriptors.clone();
            assert_eq!(engine.cpu_integer(kind, 0), Ok(StoreCompletion::Complete));
            assert_eq!(engine.arena(), expected);
            assert_eq!(engine.segment_descriptors, table);
            assert_eq!(engine.segment_descriptors.raw_descriptor(0x43), Some(&raw));
        }
    }
}

#[test]
fn inaccessible_queries_clear_zf_and_preserve_selector_and_all_other_payload_words() {
    let mut engine = engine();
    for (low, high) in [(0xffff, 0x00cf_d200), (0xdead_beef, 0x89ab_e076)] {
        engine
            .configure_segment_descriptor_raw(0x43, SegmentDescriptorRecord::from_words(low, high))
            .unwrap();
        for kind in [0x30003, 0x30004] {
            input(&mut engine, 0x43, 0x24_4ed7);
            let mut expected = engine.arena().to_vec();
            expected[TRANSFER_OFFSET + 4..TRANSFER_OFFSET + 8]
                .copy_from_slice(&0x24_4e97_u32.to_le_bytes());
            encode_cpu_result(
                0,
                0,
                Ok(()),
                &mut expected[HELPER_OFFSET..HELPER_OFFSET + 40],
            )
            .unwrap();
            let table = engine.segment_descriptors.clone();
            engine.cpu_integer(kind, 0).unwrap();
            assert_eq!(engine.arena(), expected);
            assert_eq!(engine.segment_descriptors, table);
        }
    }
    for selector in [0, 1, 2, 3, 0x53] {
        input(&mut engine, selector, 0x24_4ed7);
        engine.cpu_integer(0x30003, 0).unwrap();
        assert_eq!(
            &engine.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET + 4],
            &selector.to_le_bytes()
        );
        assert_eq!(
            &engine.arena()[TRANSFER_OFFSET + 4..TRANSFER_OFFSET + 8],
            &0x24_4e97_u32.to_le_bytes()
        );
    }
}

#[test]
fn query_protocol_and_stale_authority_refusals_are_neutral_before_publication() {
    let mut engine = engine();
    for (kind, selector, address) in [
        (0x30003, 0x23, 1),
        (0x30004, 0x10023, 0),
        (0x30005, 0x23, 0),
        (0x130003, 0x23, 0),
    ] {
        input(&mut engine, selector, 0x24_4ed7);
        let before = engine.arena().to_vec();
        let table = engine.segment_descriptors.clone();
        assert_eq!(
            engine.cpu_integer(kind, address),
            Err(HostError::InvalidRequest)
        );
        assert_eq!(engine.arena(), before);
        assert_eq!(engine.segment_descriptors, table);
    }
    input(&mut engine, 0x23, 0x24_4ed7);
    engine.protect(0x1000, 1, 3).unwrap();
    let before = engine.arena().to_vec();
    assert!(engine.cpu_integer(0x30003, 0).is_err());
    assert_eq!(engine.arena(), before);
}

#[test]
fn queries_observe_reconfigured_table_without_rewriting_loaded_segment_cache() {
    let mut engine = engine();
    engine
        .configure_segment_descriptor_raw(
            0x43,
            SegmentDescriptorRecord::from_words(0x1234, 0x0040_f200),
        )
        .unwrap();
    engine.segment_load(4, 0x43).unwrap();
    let loaded = engine.arena()[SEGMENTS_OFFSET..SEGMENTS_OFFSET + 64].to_vec();
    engine
        .configure_segment_descriptor_raw(
            0x43,
            SegmentDescriptorRecord::from_words(0x5678, 0x0080_7000),
        )
        .unwrap();
    input(&mut engine, 0x43, 2);
    engine.cpu_integer(0x30003, 0).unwrap();
    assert_eq!(
        &engine.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET + 4],
        &0x0080_7000_u32.to_le_bytes()
    );
    input(&mut engine, 0x43, 2);
    engine.cpu_integer(0x30004, 0).unwrap();
    assert_eq!(
        &engine.arena()[TRANSFER_OFFSET..TRANSFER_OFFSET + 4],
        &0x0567_8fff_u32.to_le_bytes()
    );
    assert_eq!(
        &engine.arena()[SEGMENTS_OFFSET..SEGMENTS_OFFSET + 64],
        loaded
    );
}
