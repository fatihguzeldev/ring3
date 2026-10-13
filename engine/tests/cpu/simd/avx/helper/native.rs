use ring3_engine::{
    abi::arena::TRANSFER_OFFSET,
    process::{EngineInstance, HostError, StoreCompletion},
};

#[test]
fn versioned_helper_keeps_legacy_cpu_state_and_reserved_response_contract() {
    let mut engine = EngineInstance::new(1, 91).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 2]
        .copy_from_slice(&[0xeb, 0xfe]);
    engine.upload(0x1000, 2).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8]
        .copy_from_slice(&[0, 0x10, 0, 0, 2, 0, 0, 0]);
    engine.compile(1).unwrap();
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.avx_execute(0x01000302, 0, 0, 0, 0, 2),
        Ok(StoreCompletion::Complete)
    );
    assert_eq!(&engine.arena()[..100], &before[..100]);
    assert_eq!(
        &engine.arena()[TRANSFER_OFFSET + 520..TRANSFER_OFFSET + 544],
        &[0; 24]
    );
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.avx_execute(0x02000302, 0, 0, 0, 0, 2),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(engine.arena(), before);
}
