use super::*;
use crate::windows::{
    WindowsApi32,
    memory::{MEM_COMMIT, MEM_RESERVE, MemoryApi32},
};

#[test]
fn captured_heap_and_general_virtual_memory_calls_share_the_real_kernel32_authority() {
    let mut engine = engine();
    let base = call(
        &mut engine,
        WindowsApi32::VirtualAlloc.id(),
        &[0, 128 * 1024 * 1024, MEM_RESERVE, 4],
    );
    assert_ne!(base, 0);
    assert_eq!(engine.memory.as_ref().unwrap().mapped_pages(), 3);
    assert_eq!(
        call(
            &mut engine,
            MemoryApi32::VirtualQuery.id(),
            &[base, 0x2000, 28]
        ),
        28
    );
    let mut bytes = [0; 28];
    engine
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(0x2000), &mut bytes)
        .unwrap();
    assert_eq!(
        u32::from_le_bytes(bytes[16..20].try_into().unwrap()),
        MEM_RESERVE
    );
    assert_eq!(
        call(
            &mut engine,
            WindowsApi32::VirtualAlloc.id(),
            &[base + 4096, 4096, MEM_COMMIT, 4]
        ),
        base + 4096
    );
    let heap = call(&mut engine, MemoryApi32::GetProcessHeap.id(), &[]);
    let pointer = call(&mut engine, MemoryApi32::HeapAlloc.id(), &[heap, 8, 70000]);
    assert_ne!(pointer, 0);
    assert_eq!(
        call(&mut engine, MemoryApi32::HeapSize.id(), &[heap, 0, pointer]),
        70000
    );
    assert_eq!(
        call(&mut engine, MemoryApi32::HeapFree.id(), &[heap, 0, pointer]),
        1
    );
    assert_eq!(
        call(&mut engine, MemoryApi32::HeapSize.id(), &[heap, 0, pointer]),
        u32::MAX
    );
    assert!(engine.pending_call.is_none());
}

#[test]
fn explicit_process_ex_memory_calls_reject_foreign_authority_without_allocating() {
    let mut engine = engine();
    assert_eq!(
        call(
            &mut engine,
            MemoryApi32::VirtualAllocEx.id(),
            &[7, 0, 65536, 0x3000, 4]
        ),
        0
    );
    assert_eq!(engine.windows_thread.last_error(), 6);
    assert!(engine.windows_memory.is_empty());
    assert_eq!(
        call(
            &mut engine,
            MemoryApi32::VirtualAllocEx.id(),
            &[u32::MAX, 0, 65536, 0x3000, 4]
        ),
        0x1000_0000
    );
}

#[test]
fn heap_list_output_fault_precedes_lazy_default_heap_creation() {
    let mut engine = engine();
    assert_eq!(
        call(&mut engine, MemoryApi32::GetProcessHeaps.id(), &[1, 0x7777]),
        0
    );
    assert_eq!(engine.windows_thread.last_error(), 998);
    assert!(engine.process_heap().is_none());
    assert!(engine.windows_memory.is_empty());
}

#[test]
fn initial_heap_candidate_reserves_without_touching_live_authority_or_ram() {
    let mut engine = engine();
    let mapped = engine.memory.as_ref().unwrap().mapped_pages();
    let version = engine.memory.as_ref().unwrap().transaction_version();
    let external = engine.guest_address_reservations().unwrap();
    let candidate = engine
        .windows_memory
        .prepare_initial_heap(engine.memory.as_mut().unwrap(), &external)
        .unwrap();
    assert!(candidate.default_heap().is_some());
    assert_eq!(candidate.reservation_count(), 1);
    assert!(engine.windows_memory.is_empty());
    assert_eq!(engine.memory.as_ref().unwrap().mapped_pages(), mapped);
    assert_eq!(
        engine.memory.as_ref().unwrap().transaction_version(),
        version
    );
}
