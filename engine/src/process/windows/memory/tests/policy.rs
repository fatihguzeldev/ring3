use super::*;
use crate::windows::memory::MemoryApi32 as Api;

fn word(engine: &EngineInstance, address: u32) -> u32 {
    let mut bytes = [0; 4];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(address), &mut bytes)
        .unwrap();
    u32::from_le_bytes(bytes)
}
#[test]
fn heap_information_publishes_exact_mode_and_required_length_with_neutral_invalid_output() {
    let mut engine = engine();
    let heap = call(&mut engine, Api::GetProcessHeap.id(), &[]);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1800), &[0xaa; 16])
        .unwrap();
    assert_eq!(
        call(
            &mut engine,
            Api::HeapQueryInformation.id(),
            &[heap, 0, 0x1800, 0, 0x1808]
        ),
        0
    );
    assert_eq!(word(&engine, 0x1808), 4);
    assert_eq!(word(&engine, 0x1800), 0xaaaaaaaa);
    assert_eq!(engine.current_windows_thread().unwrap().last_error(), 122);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1800), &2u32.to_le_bytes())
        .unwrap();
    assert_eq!(
        call(
            &mut engine,
            Api::HeapSetInformation.id(),
            &[heap, 0, 0x1800, 4]
        ),
        1
    );
    assert_eq!(
        call(
            &mut engine,
            Api::HeapQueryInformation.id(),
            &[heap, 0, 0x1804, 4, 0x1808]
        ),
        1
    );
    assert_eq!(word(&engine, 0x1804), 2);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1804), &0x55555555u32.to_le_bytes())
        .unwrap();
    assert_eq!(
        call(
            &mut engine,
            Api::HeapQueryInformation.id(),
            &[heap, 0, 0x1804, 4, 0xffff_fffe]
        ),
        0
    );
    assert_eq!(word(&engine, 0x1804), 0x55555555);
    assert_eq!(engine.current_windows_thread().unwrap().last_error(), 998);
}
#[test]
fn x86_slist_initialization_zeroes_exactly_eight_aligned_bytes() {
    let mut engine = engine();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1800), &[0xaa; 16])
        .unwrap();
    call(&mut engine, Api::InitializeSListHead.id(), &[0x1800]);
    let mut bytes = [0; 16];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x1800), &mut bytes)
        .unwrap();
    assert_eq!(&bytes[..8], &[0; 8]);
    assert_eq!(&bytes[8..], &[0xaa; 8]);
    call(&mut engine, Api::InitializeSListHead.id(), &[0x1804]);
    let mut unchanged = [0; 16];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x1800), &mut unchanged)
        .unwrap();
    assert_eq!(unchanged, bytes);
    assert_eq!(engine.current_windows_thread().unwrap().last_error(), 87);
}
