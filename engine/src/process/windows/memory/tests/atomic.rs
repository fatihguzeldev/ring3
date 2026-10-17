use super::*;
use crate::windows::memory::MemoryApi32 as Api;
#[test]
fn interlocked_wraps_and_exchanges_with_canonical_code_invalidation() {
    let mut engine = engine();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x3000), &u32::MAX.to_le_bytes())
        .unwrap();
    let snapshot = engine
        .memory()
        .unwrap()
        .snapshot_code(GuestAddress(0x3000), 4)
        .unwrap();
    assert_eq!(
        engine
            .memory_interlocked(Api::InterlockedIncrement, &[0x3000])
            .unwrap(),
        0
    );
    assert!(!engine.memory().unwrap().is_code_current(&snapshot));
    assert_eq!(
        engine
            .memory_interlocked(Api::InterlockedDecrement, &[0x3000])
            .unwrap(),
        u32::MAX
    );
    assert_eq!(
        engine
            .memory_interlocked(Api::InterlockedExchange, &[0x3000, 42])
            .unwrap(),
        u32::MAX
    );
    let mut bytes = [0; 4];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x3000), &mut bytes)
        .unwrap();
    assert_eq!(u32::from_le_bytes(bytes), 42);
}
#[test]
fn captured_interlocked_call_preserves_its_destination_when_it_aliases_teb_last_error() {
    let mut engine = EngineInstance::new(128, KEY).unwrap();
    let image = crate::process::windows::process::test_image_for_startup();
    engine.register_guest_module("atomic.exe", &image).unwrap();
    engine
        .start_module_process("atomic.exe", None, Default::default())
        .unwrap();
    engine.map(0x1000, 2, 3).unwrap();
    engine.map(0x3000, 1, 7).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x3000), &[0x0f, 0x0b])
        .unwrap();
    let last_error = engine.module_teb_address().unwrap() + 0x34;
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(last_error), &17u32.to_le_bytes())
        .unwrap();
    assert_eq!(
        call(&mut engine, Api::InterlockedIncrement.id(), &[last_error]),
        18
    );
    let mut bytes = [0; 4];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(last_error), &mut bytes)
        .unwrap();
    assert_eq!(u32::from_le_bytes(bytes), 18);
    assert_eq!(engine.current_windows_thread().unwrap().last_error(), 18);
    assert_eq!(
        call(
            &mut engine,
            Api::InterlockedExchange.id(),
            &[last_error, 42]
        ),
        18
    );
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(last_error), &mut bytes)
        .unwrap();
    assert_eq!(u32::from_le_bytes(bytes), 42);
}
