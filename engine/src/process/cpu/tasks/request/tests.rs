use super::*;
use crate::abi::tasks::encode_task_return_sources;
use crate::cpu::x86::{SegmentDescriptorRecord, tasks::TaskRegister};

#[test]
fn configuration_guard_precedes_payload_decode_and_configured_query_images_are_neutral() {
    let mut engine = EngineInstance::new(1, 0x5453).unwrap();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 32].fill(0xff);
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.configure_task_return_sources_request(),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(engine.arena(), before);
    let current = TaskRegister::from_cached(
        0x48,
        SegmentDescriptorRecord::from_words(0x50000067, 0x8b00),
    )
    .unwrap();
    encode_task_return_sources(
        current,
        0x5f,
        &mut engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 32],
    )
    .unwrap();
    let before = engine.arena().to_vec();
    engine.configure_task_return_sources_request().unwrap();
    assert_eq!(engine.arena(), before);
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 32].fill(0xff);
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.configure_task_return_sources_request(),
        Err(HostError::InvalidRequest)
    );
    assert_eq!(engine.arena(), before);
}
