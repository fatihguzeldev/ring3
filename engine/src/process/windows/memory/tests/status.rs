use super::*;
use crate::windows::memory::MemoryApi32 as Api;
#[test]
fn global_status_reports_real_resident_capacity_and_usage() {
    let mut engine = engine();
    engine.global_memory_status(0x1800).unwrap();
    let mut bytes = [0; 32];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x1800), &mut bytes)
        .unwrap();
    let word = |i: usize| u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap());
    assert_eq!(word(0), 32);
    assert_eq!(word(2), 64 * 4096);
    assert_eq!(word(3), 61 * 4096);
}

#[test]
fn virtual_memory_status_counts_reserved_overlap_and_each_alias_once() {
    use crate::memory::{PageRange, Permissions};
    fn available(engine: &mut EngineInstance) -> u32 {
        engine.global_memory_status(0x1800).unwrap();
        let mut bytes = [0; 4];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(0x181c), &mut bytes)
            .unwrap();
        u32::from_le_bytes(bytes)
    }
    let mut engine = engine();
    let initial = available(&mut engine);
    engine
        .windows_memory
        .reserve_for_test(
            engine.memory.as_mut().unwrap(),
            PageRange::new(GuestAddress(0x40000), 3).unwrap(),
        )
        .unwrap();
    assert_eq!(available(&mut engine), initial - 3 * 4096);
    engine.map(0x40000, 1, 3).unwrap();
    assert_eq!(available(&mut engine), initial - 3 * 4096);
    let memory = engine.memory.as_mut().unwrap();
    let pages = memory.allocate_shared_pages(1, &[]).unwrap();
    for address in [0x80000, 0x90000] {
        memory
            .map_shared_pages(
                PageRange::new(GuestAddress(address), 1).unwrap(),
                &pages,
                0,
                Permissions::READ_WRITE,
                false,
            )
            .unwrap();
    }
    assert_eq!(available(&mut engine), initial - 5 * 4096);
    engine.unmap(0x80000, 1).unwrap();
    assert_eq!(available(&mut engine), initial - 4 * 4096);
    engine
        .memory
        .as_mut()
        .unwrap()
        .release_shared_pages(pages)
        .unwrap();
}

#[test]
fn captured_global_status_output_can_alias_teb_last_error_without_being_rewritten() {
    let mut engine = EngineInstance::new(128, KEY).unwrap();
    let image = crate::process::windows::process::test_image_for_startup();
    engine.register_guest_module("status.exe", &image).unwrap();
    engine
        .start_module_process("status.exe", None, Default::default())
        .unwrap();
    engine.map(0x1000, 2, 3).unwrap();
    engine.map(0x3000, 1, 7).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x3000), &[0x0f, 0x0b])
        .unwrap();
    let output = engine.module_teb_address().unwrap() + 0x34;
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(output), &17u32.to_le_bytes())
        .unwrap();
    assert_eq!(
        call(&mut engine, Api::GlobalMemoryStatus.id(), &[output]),
        0
    );
    let mut bytes = [0; 32];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(output), &mut bytes)
        .unwrap();
    assert_eq!(u32::from_le_bytes(bytes[..4].try_into().unwrap()), 32);
    assert_eq!(
        u32::from_le_bytes(bytes[8..12].try_into().unwrap()),
        128 * 4096
    );
    assert_eq!(engine.current_windows_thread().unwrap().last_error(), 32);
}
