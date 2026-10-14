use super::*;
use crate::{
    abi::{
        arena::{SEGMENTS_OFFSET, TRANSFER_OFFSET},
        x86::{SEGMENTS_SIZE, encode_current_segment_context},
    },
    cpu::x86::SegmentCache,
};
fn engine() -> EngineInstance {
    let mut e = EngineInstance::new(2, 7).unwrap();

    e.map(0x1000, 1, 7).unwrap();

    e.memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1000), &[0xeb, 0xfe])
        .unwrap();
    e.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0, 0x10, 0, 0, 2, 0, 0, 0]);
    e.compile(1).unwrap();
    e
}
fn without_packet(e: &EngineInstance) -> Vec<u8> {
    let mut arena = e.arena().to_vec();
    arena[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE].fill(0);
    arena
}
#[test]
fn bound_fetch_detects_execute_pf_without_instruction_progress_and_cs_fault_has_priority() {
    let mut e = engine();
    let before = without_packet(&e);
    e.cpu_fetch_fault(0x5000, 1).unwrap();
    assert_eq!(e.cr2(), Some(GuestAddress(0x5000)));
    assert_eq!(without_packet(&e), before);
    let mut context = e.segment_context().unwrap();
    context.caches[1] = SegmentCache::from_bits(0xf2b0_1000).unwrap();
    let mut segment = [0; SEGMENTS_SIZE];
    let mut bases = [0; crate::abi::x86::SEGMENT_BASES_RECORD_SIZE];
    encode_current_segment_context(&context, &mut segment, &mut bases).unwrap();
    e.arena_mut().unwrap()[SEGMENTS_OFFSET..SEGMENTS_OFFSET + SEGMENTS_SIZE]
        .copy_from_slice(&segment);
    let before = without_packet(&e);
    e.cpu_fetch_fault(0x6000, 1).unwrap();
    assert_eq!(e.cr2(), Some(GuestAddress(0x5000)));
    assert_eq!(without_packet(&e), before);
    let before = e.arena().to_vec();
    assert_eq!(e.cpu_fetch_fault(0x1000, 0), Err(HostError::InvalidRequest));
    assert_eq!(e.arena(), before);
}
#[test]
fn repaired_fetch_returns_success_without_reusing_old_fault_and_wrong_owner_is_neutral() {
    let mut e = engine();
    e.cpu_fetch_fault(0x5000, 1).unwrap();
    e.map(0x5000, 1, 7).unwrap();

    let before = without_packet(&e);
    e.cpu_fetch_fault(0x5000, 1).unwrap();
    assert_eq!(e.cr2(), Some(GuestAddress(0x5000)));
    assert_eq!(without_packet(&e), before);
    assert_eq!(
        u32::from_le_bytes(
            e.arena()[HELPER_OFFSET + 16..HELPER_OFFSET + 20]
                .try_into()
                .unwrap()
        ),
        0
    );
    let before = e.arena().to_vec();
    assert_eq!(
        e.cpu_fetch_fault_resident(e.key + 1, 1, 0x6000, 1),
        Err(HostError::InvalidArtifact)
    );
    assert_eq!(e.arena(), before);
    assert_eq!(e.cr2(), Some(GuestAddress(0x5000)));
}

#[test]
fn application_execute_denial_is_a_precise_protection_fault() {
    let mut e = engine();
    e.map(0x5000, 1, 3).unwrap();

    let before = without_packet(&e);
    e.cpu_fetch_fault(0x5000, 1).unwrap();
    let mut expected = [0; 40];
    expected[..16].copy_from_slice(b"R3MH\x0f\0\x01\0\x28\0\0\0\0\0\0\0");
    for (i, value) in [9_u32, 0, 21, 0x5000, 3, 1].into_iter().enumerate() {
        expected[16 + i * 4..20 + i * 4].copy_from_slice(&value.to_le_bytes());
    }
    assert_eq!(&e.arena()[HELPER_OFFSET..HELPER_OFFSET + 40], &expected);
    assert_eq!(e.cr2(), Some(GuestAddress(0x5000)));
    assert_eq!(without_packet(&e), before);
}
