use super::*;
use crate::{
    cpu::x86::{CpuPagingError32, PagingAccess32},
    memory::GuestAddress,
};

fn engine() -> EngineInstance {
    let mut engine = EngineInstance::new(1, 7).unwrap();
    engine.map(0x1000, 1, 7).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1000), &[0x12, 0x34, 0x56, 0x78])
        .unwrap();
    engine
}

fn snapshot(engine: &EngineInstance) -> (Vec<u8>, [u8; 4], u64) {
    let memory = engine.memory.as_ref().unwrap();
    let mut bytes = [0; 4];
    memory.read(GuestAddress(0x1000), &mut bytes).unwrap();
    (
        engine.arena().to_vec(),
        bytes,
        memory.code_validation_version(),
    )
}

#[test]
fn cr2_query_distinguishes_unknown_from_known_zero_and_only_writes_four_transfer_bytes() {
    let mut engine = engine();
    engine.arena_mut().unwrap()[TRANSFER_OFFSET..TRANSFER_OFFSET + 16].fill(0xa5);
    let before = snapshot(&engine);
    assert_eq!(engine.read_cr2_request(), Err(HostError::InvalidRequest));
    assert_eq!(snapshot(&engine), before);
    for (linear, expected) in [(0, [0, 0, 0, 0]), (0xfffff000, [0, 0xf0, 0xff, 0xff])] {
        let error = engine
            .preflight_cpu_paging32(GuestAddress(linear), 1, PagingAccess32::Read)
            .unwrap_err();
        assert!(matches!(error, CpuPagingError32::PageFault(_)));
        assert_eq!(engine.cr2(), Some(GuestAddress(linear)));
        let mut expected_arena = engine.arena().to_vec();
        expected_arena[TRANSFER_OFFSET..TRANSFER_OFFSET + 4].copy_from_slice(&expected);
        engine.read_cr2_request().unwrap();
        assert_eq!(engine.arena(), expected_arena);
        assert_eq!(
            &engine.arena()[TRANSFER_OFFSET + 4..TRANSFER_OFFSET + 16],
            &[0xa5; 12]
        );
    }
}

#[test]
fn request_lifetime_precedes_unknown_cr2_without_touching_diagnostics() {
    for terminal in [HostError::Closed, HostError::ProcessExited] {
        let mut engine = engine();
        match terminal {
            HostError::Closed => engine.close(),
            HostError::ProcessExited => engine.exit_code = Some(7),
            _ => unreachable!(),
        }
        let before = engine.arena().to_vec();
        assert_eq!(engine.read_cr2_request(), Err(terminal));
        assert_eq!(engine.arena(), before);
        assert_eq!(engine.cr2(), None);
    }
}
