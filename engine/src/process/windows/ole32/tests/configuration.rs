use crate::{
    memory::{GuestAddress, PageRange},
    process::{EngineInstance, HostError, Ole32Config32},
};
fn config(address: u32) -> Ole32Config32 {
    Ole32Config32 {
        objects: PageRange::new(GuestAddress(address), 16).unwrap(),
        max_objects: 8,
        max_threads: 4,
    }
}
#[test]
fn unmapped_rpc_reservation_needs_no_resident_pages_and_rejects_overlapping_com_objects() {
    let mut engine = EngineInstance::new(8, 7).unwrap();
    assert_eq!(
        engine.configure_ole32(config(0x67000000)),
        Err(HostError::InvalidRequest)
    );
    assert!(engine.ole32.is_none());
    assert_eq!(engine.memory().unwrap().mapped_pages(), 0);
    engine.configure_ole32(config(0x68000000)).unwrap();
    let reservation = engine.ole32.as_ref().unwrap().rpc_buffer_reservation();
    assert_eq!(
        reservation,
        PageRange::new(GuestAddress(0x67000000), 4096).unwrap()
    );
    assert_eq!(engine.memory().unwrap().mapped_pages(), 0);
}
#[test]
fn preexisting_guest_ram_inside_rpc_arena_is_not_adopted_or_changed() {
    let mut engine = EngineInstance::new(8, 7).unwrap();
    engine.map(0x67001000, 1, 3).unwrap();
    engine.write32(0x67001000, 0xaabbccdd).unwrap();
    let before = engine.memory().unwrap().transaction_version();
    assert_eq!(
        engine.configure_ole32(config(0x68000000)),
        Err(HostError::InvalidRequest)
    );
    assert!(engine.ole32.is_none());
    assert_eq!(engine.memory().unwrap().transaction_version(), before);
    assert_eq!(engine.memory().unwrap().mapped_pages(), 1);
    let mut bytes = [0; 4];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x67001000), &mut bytes)
        .unwrap();
    assert_eq!(bytes, 0xaabbccddu32.to_le_bytes());
}
