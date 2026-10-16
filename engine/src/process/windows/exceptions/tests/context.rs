use super::*;
use crate::{cpu::x86::SegmentDescriptorRecord, windows::exceptions::{ExceptionError32, context::{CONTEXT_CONTROL32, Win32Context32}}};

#[test]
fn audit_context_cs_validates_the_requested_instruction_pointer_in_both_directions() {
    let mut engine = engine();
    engine.segment_descriptors.configure_raw(0x40, SegmentDescriptorRecord::from_words(0xff, 0x0040_fa00)).unwrap();
    let mut base = engine.windows_context_state().unwrap();
    for (old, requested, valid) in [(0x401000u32, 0x40u32, true), (0x40, 0x100, false)] {
        base.state.eip = old;
        let captured = Win32Context32::capture(CONTEXT_CONTROL32, &base).unwrap();
        let mut bytes = *captured.bytes();
        bytes[184..188].copy_from_slice(&requested.to_le_bytes());
        bytes[188..192].copy_from_slice(&0x43u32.to_le_bytes());
        let request = Win32Context32::from_bytes(&bytes).unwrap();
        let result = engine.apply_windows_context(&request, &base);
        assert_eq!(result.is_ok(), valid, "old={old:#x}, requested={requested:#x}");
        if let Ok(state) = result { assert_eq!(state.state.eip, requested); }
    }
}

#[test]
fn audit_nonzero_debug_context_cannot_silently_resume_and_the_same_handler_can_retry() {
    let mut engine = engine();
    engine.exceptions.handlers.add(false, true, 0x401000).unwrap();
    let original = engine.windows_context_state().unwrap();
    let dispatch = engine.begin_exception_delivery(
        ExceptionRecord32::new(0xe0000001, 0, original.state.eip, &[]).unwrap(),
        original, PendingOwner::Replacement(0), None, None, None,
    ).unwrap();
    engine.memory.as_mut().unwrap().write(GuestAddress(dispatch.context + 4), &0x401000u32.to_le_bytes()).unwrap();
    returned(&mut engine, dispatch, u32::MAX, false);
    let arena = engine.arena().to_vec();
    let version = engine.memory().unwrap().transaction_version();
    assert_eq!(engine.finish_windows_exception_handler(77, dispatch.id), Err(crate::process::HostError::Exception(ExceptionError32::InvalidContext)));
    assert_eq!(engine.arena(), arena);
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    assert_eq!(engine.pending_windows_exception().unwrap().id, dispatch.id);
    engine.memory.as_mut().unwrap().write(GuestAddress(dispatch.context + 4), &0u32.to_le_bytes()).unwrap();
    assert_eq!(engine.finish_windows_exception_handler(77, dispatch.id).unwrap(), None);
    assert_eq!(engine.windows_context_state().unwrap().debug, [0; 6]);
}
