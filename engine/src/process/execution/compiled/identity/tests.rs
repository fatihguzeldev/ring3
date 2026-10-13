use crate::{
    abi::arena::TRANSFER_OFFSET,
    process::{EngineInstance, HostError},
};

#[test]
fn last_generation_installs_once_and_never_wraps_or_replaces() {
    let mut instance = EngineInstance::new(1, 1).unwrap();
    instance.map(0x1000, 1, 7).unwrap();
    instance.arena_mut().unwrap()[TRANSFER_OFFSET] = 0x90;
    instance.upload(0x1000, 1).unwrap();
    let descriptor = &mut instance.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 8];
    descriptor[..4].copy_from_slice(&0x1000_u32.to_le_bytes());
    descriptor[4..].copy_from_slice(&1_u32.to_le_bytes());
    instance.generation = u32::MAX - 1;
    assert_eq!(instance.compile(1), Ok(u32::MAX));
    let bytes = instance.artifact_bytes().unwrap().to_vec();
    assert_eq!(instance.compile(1), Err(HostError::GenerationExhausted));
    assert_eq!(instance.generation(), u32::MAX);
    assert_eq!(instance.artifact_bytes().unwrap(), bytes);
    assert_eq!(instance.guard(1, u32::MAX), Ok(()));
    assert_eq!(instance.guard(1, 0), Err(HostError::InvalidArtifact));
}
