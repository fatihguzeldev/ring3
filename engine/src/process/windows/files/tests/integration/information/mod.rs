mod metadata;
mod namespace;

use super::*;
use crate::windows::{WindowsApi32, files::FileApi32};
const SET_INFORMATION: u32 = 0x000a_00b3;
fn write(engine: &mut EngineInstance, address: u32, bytes: &[u8]) {
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(address), bytes)
        .unwrap();
}
fn set(engine: &mut EngineInstance, handle: u32, class: u32, address: u32, size: u32) -> u32 {
    complete(engine, SET_INFORMATION, &[handle, class, address, size])
}
fn error(engine: &mut EngineInstance) -> u32 {
    complete(engine, WindowsApi32::GetLastError.id(), &[])
}
fn create(engine: &mut EngineInstance, name: &str, access: u32, share: u32, flags: u32) -> u32 {
    super::super::support::path(engine.memory.as_mut().unwrap(), 0x1000, name);
    complete(
        engine,
        FileApi32::CreateFileW.id(),
        &[0x1000, access, share, 0, 4, flags, 0],
    )
}
fn info(engine: &mut EngineInstance, handle: u32, class: u32, size: u32) -> Vec<u8> {
    assert_eq!(
        complete(
            engine,
            FileApi32::GetFileInformationByHandleEx.id(),
            &[handle, class, 0x3000, size]
        ),
        1
    );
    let mut bytes = vec![0; size as usize];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x3000), &mut bytes)
        .unwrap();
    bytes
}
