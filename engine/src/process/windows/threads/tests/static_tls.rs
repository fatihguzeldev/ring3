use crate::{
    memory::GuestAddress,
    process::{EngineInstance, ModuleOperation32, ModuleRuntimeConfig32},
};
pub(super) fn image(base: u32, dll: bool, tls: bool) -> Vec<u8> {
    let mut bytes = vec![0; 1536];
    bytes[..2].copy_from_slice(b"MZ");
    bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
    for (at, value) in [
        (0x84, 0x14cu16),
        (0x86, 2),
        (0x94, 224),
        (0x96, if dll { 0x2103 } else { 0x103 }),
        (0x98, 0x10b),
        (0xdc, 3),
    ] {
        bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }
    for (at, value) in [
        (0x3c, 0x80u32),
        (0xa8, if dll { 0 } else { 0x1000 }),
        (0xb4, base),
        (0xb8, 4096),
        (0xbc, 512),
        (0xd0, 0x4000),
        (0xd4, 512),
        (0xe0, 0x100000),
        (0xe4, 4096),
        (0xf4, 16),
    ] {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    for (at, name, rva, raw, flags) in [
        (0x178, b".text\0\0\0", 0x1000u32, 512u32, 0x6000_0020u32),
        (0x1a0, b".data\0\0\0", 0x3000, 1024, 0xc000_0040),
    ] {
        bytes[at..at + 8].copy_from_slice(name);
        for (offset, value) in [(8, 512), (12, rva), (16, 512), (20, raw), (36, flags)] {
            bytes[at + offset..at + offset + 4].copy_from_slice(&value.to_le_bytes());
        }
    }
    bytes[512..516].copy_from_slice(&[0x90, 0xc2, 4, 0]);
    if tls {
        bytes[0x140..0x144].copy_from_slice(&0x3000u32.to_le_bytes());
        bytes[0x144..0x148].copy_from_slice(&24u32.to_le_bytes());
        for (index, value) in [
            base + 0x3060,
            base + 0x3064,
            base + 0x3050,
            0,
            12,
            0x0030_0000,
        ]
        .into_iter()
        .enumerate()
        {
            bytes[1024 + index * 4..1028 + index * 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes[1120..1124].copy_from_slice(&[0xaa, 0x11, 0x22, 0x33]);
    }
    bytes
}
fn word(engine: &EngineInstance, address: u32) -> u32 {
    let mut bytes = [0; 4];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(address), &mut bytes)
        .unwrap();
    u32::from_le_bytes(bytes)
}
fn block(engine: &EngineInstance, thread: u32, slot: u32) -> u32 {
    let teb = engine.windows_thread_info(thread).unwrap().teb;
    word(engine, word(engine, teb + 0x2c) + slot * 4)
}
fn template(engine: &EngineInstance, address: u32) {
    let mut bytes = [0xff; 16];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(address), &mut bytes)
        .unwrap();
    assert_eq!(
        bytes,
        [0xaa, 0x11, 0x22, 0x33, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
    );
}
#[test]
fn late_tls_dll_load_unload_reload_updates_every_child_once_and_uses_original_template() {
    let mut engine = EngineInstance::new(256, 71).unwrap();
    engine.configure_cpu_clock(1_000_000_000, 0).unwrap();
    engine
        .configure_synchronization(Default::default())
        .unwrap();
    engine.register_kernel32_module(0x7000_0000).unwrap();
    engine
        .register_guest_module("main.exe", &image(0x400000, false, false))
        .unwrap();
    engine
        .register_guest_module("late.dll", &image(0x600000, true, true))
        .unwrap();
    assert_eq!(
        engine
            .start_module_process("main.exe", None, ModuleRuntimeConfig32::default())
            .unwrap(),
        ModuleOperation32::Loaded { handle: 0x400000 }
    );
    engine.load_library_module("kernel32.dll", None).unwrap();
    let gate = engine
        .module_gates()
        .unwrap()
        .into_iter()
        .find(|gate| gate.id == super::super::THREAD_RETURN_ID)
        .unwrap()
        .entry
        .0;
    engine
        .create_windows_thread_at_gate(&[0, 0, 0x401000, 0, 4, 0], gate)
        .unwrap();
    let teb = engine.windows_thread_info(2).unwrap().teb;
    let array = word(&engine, teb + 0x2c);
    engine.write32(teb + 0x2c, 0x12345).unwrap();
    let version = engine.memory().unwrap().transaction_version();
    let pages = engine.memory().unwrap().mapped_pages();
    assert!(engine.load_library_module("late.dll", None).is_err());
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    assert_eq!(engine.memory().unwrap().mapped_pages(), pages);
    assert_eq!(engine.module_handle(Some("late.dll")).unwrap(), None);
    engine.write32(teb + 0x2c, array).unwrap();
    assert_eq!(
        engine.load_library_module("late.dll", None).unwrap(),
        ModuleOperation32::Loaded { handle: 0x600000 }
    );
    let slot = word(&engine, 0x603050);
    let first = block(&engine, 2, slot);
    assert_ne!(first, 0);
    template(&engine, first);
    let primary = engine.module_teb_address().unwrap();
    let primary_block = word(&engine, word(&engine, primary + 0x2c) + slot * 4);
    assert_ne!(primary_block, first);
    engine.write32(primary_block, 0xeeee_eeee).unwrap();
    engine
        .create_windows_thread_at_gate(&[0, 0, 0x401000, 0, 4, 0], gate)
        .unwrap();
    let original_child = block(&engine, 3, slot);
    template(&engine, original_child);
    engine.free_library_module(0x600000).unwrap();
    assert_eq!(block(&engine, 2, slot), 0);
    assert_eq!(block(&engine, 3, slot), 0);
    assert!(
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(first), &mut [0; 1])
            .is_err()
    );
    engine.load_library_module("late.dll", None).unwrap();
    let slot = word(&engine, 0x603050);
    let second = block(&engine, 2, slot);
    let other = block(&engine, 3, slot);
    template(&engine, second);
    template(&engine, other);
    assert_ne!(second, other);
    engine.terminate_windows_thread(2, 17).unwrap();
    assert!(
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(second), &mut [0; 1])
            .is_err()
    );
    template(&engine, other);
}
