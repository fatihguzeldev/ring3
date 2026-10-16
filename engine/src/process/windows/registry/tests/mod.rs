mod notifications;
mod publication;
mod values;
use super::*;
use crate::{
    memory::{GuestAddress, PageRange, Permissions},
    process::EngineInstance,
    windows::registry::Advapi32Api32 as Api,
};
fn engine() -> EngineInstance {
    let mut engine = EngineInstance::new(32, 7).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .map_zeroed(
            PageRange::new(GuestAddress(0x2000), 4).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    engine.registry = Some(RegistryRuntime32::new(7, RegistryConfig32::default()).unwrap());
    engine
}
fn put(engine: &mut EngineInstance, address: u32, bytes: &[u8]) {
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(address), bytes)
        .unwrap();
}
fn word(engine: &EngineInstance, address: u32) -> u32 {
    memory::dword(engine.memory.as_ref().unwrap(), address).unwrap()
}
fn create(engine: &mut EngineInstance) -> u32 {
    put(engine, 0x2000, b"Software\\Game\0");
    assert_eq!(
        engine.registry_call(
            Api::RegCreateKeyExA,
            &[0x80000001, 0x2000, 0, 0, 0, 0xf003f, 0, 0x2100, 0x2104],
            100
        ),
        Ok(0)
    );
    word(engine, 0x2100)
}
