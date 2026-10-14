use super::*;

pub(super) fn engine() -> EngineInstance {
    let mut engine = EngineInstance::new(4, 0x464152).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.map(0x7000, 1, 3).unwrap();
    engine.arena_mut().unwrap()[140..142].copy_from_slice(&[0xeb, 0xfe]);
    engine.upload(0x1000, 2).unwrap();
    engine.arena_mut().unwrap()[140..148].copy_from_slice(&[0, 0x10, 0, 0, 2, 0, 0, 0]);
    engine.compile(1).unwrap();
    engine
        .configure_segment_descriptor_raw(
            0x43,
            crate::cpu::x86::SegmentDescriptorRecord::from_words(0xffff, 0x00cff800),
        )
        .unwrap();
    engine
}

pub(super) fn only_packet_changed(before: &[u8], engine: &EngineInstance) {
    let mut expected = before.to_vec();
    expected[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE]
        .copy_from_slice(&engine.arena()[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE]);
    assert_eq!(engine.arena(), expected);
}

pub(super) fn field(engine: &EngineInstance, offset: usize) -> u32 {
    u32::from_le_bytes(
        engine.arena()[HELPER_OFFSET + offset..HELPER_OFFSET + offset + 4]
            .try_into()
            .unwrap(),
    )
}

pub(super) fn page_fault_packet(engine: &EngineInstance, fields: [u32; 6]) {
    let mut expected = [0; HELPER_SIZE];
    expected[..16].copy_from_slice(&[82, 51, 77, 72, 15, 0, 1, 0, 40, 0, 0, 0, 0, 0, 0, 0]);
    for (index, value) in fields.into_iter().enumerate() {
        expected[16 + index * 4..20 + index * 4].copy_from_slice(&value.to_le_bytes());
    }
    assert_eq!(
        &engine.arena()[HELPER_OFFSET..HELPER_OFFSET + HELPER_SIZE],
        expected
    );
    assert_eq!(engine.cr2(), Some(crate::memory::GuestAddress(fields[3])));
}
