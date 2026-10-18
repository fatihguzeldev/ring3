use super::*;
use crate::{
    process::HostError,
    windows::memory::{MemoryError32, MemoryLimits32},
};

fn limits() -> MemoryLimits32 {
    MemoryLimits32 {
        regions: 16,
        fragments: 32,
        heaps: 4,
        blocks: 128,
        moveable_handles: 8,
        heap_reserve_bytes: 2 * 65536,
    }
}

#[test]
fn configuration_accepts_all_memory_limits_without_mapping_guest_pages() {
    let mut engine = engine();
    let version = engine.memory().unwrap().transaction_version();
    let arena = engine.arena().to_vec();
    engine.configure_windows_memory(limits()).unwrap();
    assert_eq!(engine.windows_memory.limits, limits());
    assert!(engine.windows_memory.is_empty());
    assert_eq!(engine.process_heap(), None);
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    assert_eq!(engine.arena(), arena);
}

#[test]
fn invalid_memory_configuration_preserves_the_previous_policy_and_guest_state() {
    let mut engine = engine();
    engine.configure_windows_memory(limits()).unwrap();
    let version = engine.memory().unwrap().transaction_version();
    let arena = engine.arena().to_vec();
    let invalid = [
        MemoryLimits32 {
            regions: 0,
            ..limits()
        },
        MemoryLimits32 {
            fragments: 15,
            ..limits()
        },
        MemoryLimits32 {
            heaps: 0,
            ..limits()
        },
        MemoryLimits32 {
            blocks: 0,
            ..limits()
        },
        MemoryLimits32 {
            moveable_handles: 0,
            ..limits()
        },
        MemoryLimits32 {
            heap_reserve_bytes: 65537,
            ..limits()
        },
    ];
    for candidate in invalid {
        assert!(matches!(
            engine.configure_windows_memory(candidate),
            Err(HostError::WindowsMemory(MemoryError32::InvalidLimits))
        ));
        assert_eq!(engine.windows_memory.limits, limits());
        assert!(engine.windows_memory.is_empty());
        assert_eq!(engine.process_heap(), None);
        assert_eq!(engine.memory().unwrap().transaction_version(), version);
        assert_eq!(engine.arena(), arena);
    }
}

#[test]
fn busy_memory_configuration_preserves_heap_ownership_and_live_allocation() {
    let mut engine = engine();
    engine.configure_windows_memory(limits()).unwrap();
    let pointer = engine
        .allocate_windows_runtime_initialized(b"live allocation")
        .unwrap();
    let heap = engine.process_heap();
    let identity = engine.windows_runtime_allocation_identity(pointer).unwrap();
    let version = engine.memory().unwrap().transaction_version();
    let reservations = engine.windows_memory.reservations().collect::<Vec<_>>();
    let arena = engine.arena().to_vec();
    assert!(matches!(
        engine.configure_windows_memory(MemoryLimits32::default()),
        Err(HostError::WindowsMemory(MemoryError32::Busy))
    ));
    assert_eq!(engine.windows_memory.limits, limits());
    assert_eq!(engine.process_heap(), heap);
    assert_eq!(
        engine.windows_runtime_allocation_identity(pointer).unwrap(),
        identity
    );
    assert_eq!(
        engine.windows_memory.reservations().collect::<Vec<_>>(),
        reservations
    );
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    assert_eq!(engine.arena(), arena);
    let mut bytes = [0; 15];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(pointer), &mut bytes)
        .unwrap();
    assert_eq!(&bytes, b"live allocation");
}
