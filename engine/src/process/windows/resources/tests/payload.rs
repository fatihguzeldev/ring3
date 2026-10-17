use super::*;
use crate::loader::ResourceIdentifier32 as Id;

#[test]
fn frozen_neutral_resource_language_does_not_reselect_current_ui_language_for_children() {
    let mut engine = engine(false, 8);
    let initial = engine
        .resource_data_payload(BASE, &Id::Id(10), &Id::Id(7), 0, 64)
        .unwrap();
    let memory = engine.memory.as_mut().unwrap();
    for (address, bytes) in [
        (BASE + 0x209c, vec![0, 0, 2, 0]),
        (BASE + 0x20a0, 0u32.to_le_bytes().to_vec()),
        (BASE + 0x20a8, 0x409u32.to_le_bytes().to_vec()),
        (BASE + 0x20ac, 0xd0u32.to_le_bytes().to_vec()),
        (BASE + 0x20d0, 0x2184u32.to_le_bytes().to_vec()),
        (BASE + 0x20d4, 4u32.to_le_bytes().to_vec()),
        (BASE + 0x2184, vec![0xaa, 0xbb, 0xcc, 0xdd]),
    ] {
        memory.write(GuestAddress(address), &bytes).unwrap();
    }
    let automatic = engine
        .resource_data_payload(BASE, &Id::Id(10), &Id::Id(7), 0, 64)
        .unwrap();
    assert_eq!(automatic.language, 0x409);
    assert_eq!(automatic.bytes, [0xaa, 0xbb, 0xcc, 0xdd]);
    let neutral = engine
        .resource_payload_for_module(initial.module, &Id::Id(10), &Id::Id(7), 0, 64)
        .unwrap();
    assert_eq!(neutral.module, initial.module);
    assert_eq!(neutral.entry, initial.entry);
    assert_eq!(neutral.language, 0);
    assert_eq!(neutral.bytes, [0x11, 0x22, 0x33, 0x44]);
}
