use ring3_engine::memory::GuestAddress;
use ring3_engine::process::{EngineInstance, HostError};

fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn helper(engine: &EngineInstance) -> &[u8] {
    &engine.arena()[100..140]
}

fn assert_header(bytes: &[u8]) {
    assert_eq!(bytes.len(), 40);
    assert_eq!(
        &bytes[..16],
        &[0x52, 0x33, 0x4d, 0x48, 1, 0, 1, 0, 40, 0, 0, 0, 0, 0, 0, 0]
    );
}

#[test]
fn helpers_transfer_little_endian_u32_across_pages_and_at_final_address() {
    let mut engine = EngineInstance::new(3, 1).unwrap();
    engine.map(0x1000, 2, 3).unwrap();
    engine.map(0xffff_f000, 1, 3).unwrap();
    for address in [0x1001, 0x1ffe, 0xffff_fffc] {
        engine.write32(address, 0x4433_2211).unwrap();
        assert_header(helper(&engine));
        assert_eq!(word(helper(&engine), 16), 0);
        assert_eq!(word(helper(&engine), 20), 0);
        let mut bytes = [0; 4];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(address), &mut bytes)
            .unwrap();
        assert_eq!(bytes, [0x11, 0x22, 0x33, 0x44]);
        engine.read32(address).unwrap();
        assert_eq!(word(helper(&engine), 20), 0x4433_2211);
    }
}

#[test]
fn cross_page_write_fault_is_encoded_and_preserves_both_data_and_snapshot() {
    let mut engine = EngineInstance::new(2, 1).unwrap();
    engine.map(0x1000, 2, 7).unwrap();
    engine.write32(0x1ffe, 0x4433_2211).unwrap();
    engine.protect(0x2000, 1, 5).unwrap();
    let snapshot = engine
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(0x1ffe), 4)
        .unwrap();
    engine.write32(0x1ffe, 0xffff_ffff).unwrap();
    assert_eq!(
        [
            word(helper(&engine), 16),
            word(helper(&engine), 20),
            word(helper(&engine), 24),
            word(helper(&engine), 28),
            word(helper(&engine), 32),
            word(helper(&engine), 36)
        ],
        [1, 0, 2, 0x2000, 2, 4]
    );
    let mut bytes = [0; 4];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x1ffe), &mut bytes)
        .unwrap();
    assert_eq!(bytes, [0x11, 0x22, 0x33, 0x44]);
    assert!(engine.memory().unwrap().is_code_current(&snapshot));
}

#[test]
fn unmapped_and_overflow_helpers_replace_stale_value_with_canonical_fault() {
    let mut engine = EngineInstance::new(1, 1).unwrap();
    engine.map(0x1000, 1, 3).unwrap();
    engine.write32(0x1000, u32::MAX).unwrap();
    engine.read32(0x1000).unwrap();
    assert_eq!(word(helper(&engine), 20), u32::MAX);
    for (address, reason, fault_address) in [(0x1ffe, 1, 0x2000), (0xffff_fffd, 3, 0xffff_fffd)] {
        engine.read32(address).unwrap();
        assert_eq!(
            [
                word(helper(&engine), 16),
                word(helper(&engine), 20),
                word(helper(&engine), 24),
                word(helper(&engine), 28),
                word(helper(&engine), 32),
                word(helper(&engine), 36)
            ],
            [1, 0, reason, fault_address, 1, 4]
        );
    }
}

#[test]
fn helper_code_writes_invalidate_artifact_but_helpers_remain_lifecycle_usable() {
    let mut engine = EngineInstance::new(2, 7).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.write32(0x1000, 0x9090_9090).unwrap();
    engine.arena_mut().unwrap()[140..148].copy_from_slice(&[0, 0x10, 0, 0, 1, 0, 0, 0]);
    let generation = engine.compile(1).unwrap();
    engine.map(0x8000, 1, 3).unwrap();
    engine.write32(0x8000, 123).unwrap();
    engine.guard(7, generation).unwrap();
    engine.write32(0x1000, 0x9090_9090).unwrap();
    assert!(matches!(
        engine.guard(7, generation),
        Err(HostError::CodeInvalidated)
    ));
    engine.read32(0x1000).unwrap();
    assert_eq!(word(helper(&engine), 20), 0x9090_9090);
}
