use super::*;
use crate::{
    abi::{
        arena::{SEGMENTS_OFFSET, TRANSFER_OFFSET},
        x86::{decode_segment_context, encode_segment_context},
    },
    cpu::x86::{SegmentCache, SegmentDescriptorRecord},
};

fn engine() -> EngineInstance {
    let mut engine = EngineInstance::new(4, 0x464152).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
        .copy_from_slice(&[0xeb, 0xfe]);
    engine.upload(0x1000, 2).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0, 0x10, 0, 0, 2, 0, 0, 0]);
    engine.compile(1).unwrap();
    engine
}
fn raw(high: u32) -> SegmentDescriptorRecord {
    SegmentDescriptorRecord::from_words(0xffff, high)
}
fn value(engine: &EngineInstance, offset: usize) -> u32 {
    u32::from_le_bytes(
        engine.arena()[HELPER_OFFSET + offset..HELPER_OFFSET + offset + 4]
            .try_into()
            .unwrap(),
    )
}

#[test]
fn successful_far_jump_commits_only_packet_cache_and_accessed_after_capture_guard() {
    let mut engine = engine();
    engine
        .configure_segment_descriptor_raw(0x43, raw(0x00cff800))
        .unwrap();
    let before = engine.arena().to_vec();
    let version = engine.memory().unwrap().code_validation_version();
    engine.far_jump(0xffff1234, 0x40, 1).unwrap();
    assert_eq!(value(&engine, 16), 0);
    assert_eq!(value(&engine, 20), 0x1234);
    let mut expected = before;
    expected[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE]
        .copy_from_slice(&engine.arena()[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE]);
    expected[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE]
        .copy_from_slice(&engine.arena()[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE]);
    assert_eq!(engine.arena(), expected);
    assert_eq!(engine.segment_context().unwrap().state.selectors[1], 0x43);
    assert_eq!(
        engine.segment_context().unwrap().caches[1].bits() >> 20 & 0xf,
        9
    );
    assert_eq!(
        engine
            .segment_descriptors
            .raw_descriptor(0x43)
            .unwrap()
            .descriptor_type(),
        9
    );
    assert_eq!(engine.memory().unwrap().code_validation_version(), version);
    engine.guard(engine.key, engine.generation()).unwrap();
    let loaded = engine.segment_context().unwrap();
    engine
        .configure_segment_descriptor_raw(0x43, raw(0x00cf1200))
        .unwrap();
    assert_eq!(engine.segment_context().unwrap(), loaded);
    engine.far_jump(0x2000, 0x43, 0).unwrap();
    assert_eq!(value(&engine, 16), 3);
    assert_eq!(value(&engine, 24), 0x40);
    assert_eq!(engine.segment_context().unwrap(), loaded);
}

#[test]
fn architectural_faults_never_publish_cs_or_accessed() {
    let mut engine = engine();
    for (selector, high, status, code, offset) in [
        (0, 0x00cffa00, 3, 0, 0),
        (0x4b, 0x00cffa00, 3, 0x48, 0),
        (0x43, 0x00cff200, 3, 0x40, 0),
        (0x43, 0x00cfda00, 3, 0x40, 0),
        (0x43, 0x00cf7a00, 6, 0x40, 0),
        (0x43, 0x0040fa00, 3, 0, 0x10000),
    ] {
        engine
            .configure_segment_descriptor_raw(0x43, raw(high))
            .unwrap();
        let before = engine.arena().to_vec();
        let table = engine.segment_descriptors.clone();
        engine.far_jump(offset, selector, 0).unwrap();
        assert_eq!((value(&engine, 16), value(&engine, 24)), (status, code));
        let mut expected = before;
        expected[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE]
            .copy_from_slice(&engine.arena()[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE]);
        assert_eq!(engine.arena(), expected);
        assert_eq!(engine.segment_descriptors, table);
    }
}

#[test]
fn valid_unsupported_profiles_and_malformed_requests_are_neutral_host_refusals() {
    let mut engine = engine();
    for (low, high) in [
        (0xffff, 0x0040fa00),
        (0xffff, 0x008ffa00),
        (0x1000ffff, 0x00cffa00),
        (0xffff, 0x00effa00),
        (0xdeadbeef, 0x89abec76),
    ] {
        engine
            .configure_segment_descriptor_raw(0x43, SegmentDescriptorRecord::from_words(low, high))
            .unwrap();
        let before = engine.arena().to_vec();
        let table = engine.segment_descriptors.clone();
        assert_eq!(engine.far_jump(0, 0x43, 0), Err(HostError::InvalidRequest));
        assert_eq!(engine.arena(), before);
        assert_eq!(engine.segment_descriptors, table);
    }
    for (selector, word) in [(0x10043, 0), (0x43, 2)] {
        let before = engine.arena().to_vec();
        assert_eq!(
            engine.far_jump(0, selector, word),
            Err(HostError::InvalidRequest)
        );
        assert_eq!(engine.arena(), before);
    }
    engine
        .configure_segment_descriptor_raw(0x43, raw(0x00cffa00))
        .unwrap();
    let mut context = engine.segment_context().unwrap();
    context.caches[1] = SegmentCache::from_bits(0xf2b0ffff).unwrap();
    let mut bytes = [0; SEGMENTS_SIZE];
    encode_segment_context(&context, &mut bytes).unwrap();
    engine.arena_mut().unwrap()[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE]
        .copy_from_slice(&bytes);
    let before = engine.arena().to_vec();
    assert_eq!(engine.far_jump(0, 0x43, 0), Err(HostError::InvalidRequest));
    assert_eq!(engine.arena(), before);
    assert!(decode_segment_context(&bytes).is_ok());
}

#[test]
fn stale_capture_and_resident_identity_reject_before_descriptor_or_packet_publication() {
    let mut engine = engine();
    engine
        .configure_segment_descriptor_raw(0x43, raw(0x00cffa00))
        .unwrap();
    let before = engine.arena().to_vec();
    let table = engine.segment_descriptors.clone();
    assert!(
        engine
            .far_jump_resident(engine.key ^ 1, 1, 0, 0x43, 0)
            .is_err()
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.segment_descriptors, table);
    engine.write32(0x1000, 0x90909090).unwrap();
    let before = engine.arena().to_vec();
    assert!(engine.far_jump(0, 0x43, 0).is_err());
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.segment_descriptors, table);
}

#[test]
fn outer_system_descriptor_faults_publish_only_the_exact_fault_packet() {
    let mut engine = engine();
    for (selector, kind, dpl, present, status, detail) in [
        (0x43_u16, 3_u32, 3_u32, true, 3, 0x40),
        (0x43, 11, 3, true, 3, 0x40),
        (0x47, 1, 3, true, 3, 0x44),
        (0x47, 9, 3, true, 3, 0x44),
        (0x43, 1, 2, true, 3, 0x40),
        (0x43, 9, 3, false, 6, 0x40),
        (0x47, 4, 2, true, 3, 0x44),
        (0x47, 5, 3, false, 6, 0x44),
        (0x47, 12, 3, false, 6, 0x44),
    ] {
        let raw = SegmentDescriptorRecord::from_words(
            0,
            kind << 8 | dpl << 13 | u32::from(present) << 15,
        );
        engine
            .configure_segment_descriptor_raw(selector, raw)
            .unwrap();
        let before = engine.arena().to_vec();
        let table = engine.segment_descriptors.clone();
        engine.far_jump(0, u32::from(selector), 0).unwrap();
        let mut packet = [0; HELPER_SIZE];
        let fault = if status == 3 {
            CpuMemoryError::GeneralProtection(detail)
        } else {
            CpuMemoryError::SegmentNotPresent(detail)
        };
        encode_cpu_result(0, 0, Err(fault), &mut packet).unwrap();
        let mut expected = before;
        expected[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE].copy_from_slice(&packet);
        assert_eq!(engine.arena(), expected);
        assert_eq!(engine.segment_descriptors, table);
    }
}

#[test]
fn far_jump_keeps_nonflat_data_bases_and_companion_caches() {
    let mut engine = support::engine();
    engine
        .set_segment_base(SegmentRegister::Ds, 0x5000)
        .unwrap();
    engine
        .set_segment_base(SegmentRegister::Es, 0x6000)
        .unwrap();
    let before = engine.segment_context().unwrap();
    engine.far_jump(0x1000, 0x43, 0).unwrap();
    let after = engine.segment_context().unwrap();
    for segment in [
        SegmentRegister::Es,
        SegmentRegister::Ss,
        SegmentRegister::Ds,
        SegmentRegister::Fs,
        SegmentRegister::Gs,
    ] {
        assert_eq!(
            after.state.bases[segment.index()],
            before.state.bases[segment.index()]
        );
        assert_eq!(
            after.state.selectors[segment.index()],
            before.state.selectors[segment.index()]
        );
        assert_eq!(
            after.caches[segment.index()],
            before.caches[segment.index()]
        );
    }
    assert_eq!(after.state.selectors[SegmentRegister::Cs.index()], 0x43);
    assert_eq!(support::field(&engine, 16), 0);
}
