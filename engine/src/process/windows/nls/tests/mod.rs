mod calendar;
mod conversion;
mod direction;
mod international;
mod locale;
mod names;
use crate::{
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    process::EngineInstance,
    windows::nls::NlsApi32,
};
fn engine() -> EngineInstance {
    let mut engine = EngineInstance::new(16, 7).unwrap();
    let mut memory = AddressSpace::new(16).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x10000), 3).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    engine.memory = Some(memory);
    engine
}
fn write(engine: &mut EngineInstance, address: u32, bytes: &[u8]) {
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(address), bytes)
        .unwrap();
}
fn read<const N: usize>(engine: &EngineInstance, address: u32) -> [u8; N] {
    let mut bytes = [0; N];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(address), &mut bytes)
        .unwrap();
    bytes
}
fn wide(engine: &mut EngineInstance, address: u32, text: &str) {
    let bytes = text
        .encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    write(engine, address, &bytes);
}
fn call(engine: &mut EngineInstance, api: NlsApi32, args: &[u32]) -> u32 {
    engine.execute_nls_call(api, args).unwrap()
}
