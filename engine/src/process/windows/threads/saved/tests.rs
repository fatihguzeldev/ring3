use crate::process::cpu::read_view::tests::fixture;

#[test]
fn nested_thread_contexts_derive_view_epoch_from_the_restored_slot() {
    let (mut engine, owner) = fixture(true);
    let outer = engine.begin_exception_detection(owner).unwrap();
    let outer_address = engine.cpu_read32_view_address(outer);
    let saved_outer = engine.take_windows_thread_context().unwrap();
    assert_eq!(engine.cpu_read32_view_address(outer), 0);
    let inner = engine.begin_exception_detection(owner).unwrap();
    assert!(inner > outer);
    assert_ne!(engine.cpu_read32_view_address(inner), 0);
    let saved_inner = engine.take_windows_thread_context().unwrap();
    assert_eq!(engine.cpu_read32_view_address(inner), 0);
    engine.restore_windows_thread_context(saved_outer);
    assert_eq!(engine.cpu_read32_view_address(outer), outer_address);
    assert_eq!(engine.cpu_read32_view_address(inner), 0);
    let _saved_outer = engine.take_windows_thread_context().unwrap();
    engine.restore_windows_thread_context(saved_inner);
    assert_ne!(engine.cpu_read32_view_address(inner), 0);
    assert_eq!(engine.cpu_read32_view_address(outer), 0);
}

#[test]
fn parked_fault_witness_cannot_be_cleared_by_the_other_thread() {
    let (mut engine, owner) = fixture(true);
    let epoch = engine.begin_exception_detection(owner).unwrap();
    engine.cpu_read32(0x6003).unwrap();
    let witness = engine.certified_page_fault32(epoch).unwrap();
    let saved = engine.take_windows_thread_context().unwrap();
    engine.clear_certified_page_fault32();
    assert_eq!(engine.cpu_read32_view_address(epoch), 0);
    engine.restore_windows_thread_context(saved);
    assert_eq!(engine.certified_page_fault32(epoch), Some(witness));
    assert_eq!(engine.cpu_read32_view_address(epoch), 0);
    engine.cpu_read32(0x7000).unwrap();
    assert_ne!(engine.cpu_read32_view_address(epoch), 0);
}

#[test]
fn failed_thread_capture_preserves_the_view_and_its_original_active_slot() {
    let (mut engine, owner) = fixture(true);
    let epoch = engine.begin_exception_detection(owner).unwrap();
    let before = engine.cpu_read32_view;
    engine.arena_mut().unwrap()[crate::abi::arena::X87_OFFSET] ^= 1;
    let arena = engine.arena().to_vec();
    assert!(engine.take_windows_thread_context().is_err());
    assert_eq!(engine.cpu_read32_view, before);
    assert_ne!(engine.cpu_read32_view_address(epoch), 0);
    assert_eq!(engine.arena(), arena);
}
