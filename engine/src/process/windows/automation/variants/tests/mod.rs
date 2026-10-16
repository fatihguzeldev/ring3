mod date;
mod ownership;
mod scalar;

use crate::{
    memory::GuestAddress,
    process::{
        EngineInstance,
        windows::automation::tests::{call, engine, read},
    },
    windows::automation::{Oleaut32Api32 as Api, variants::Variant32},
};

fn initialized() -> EngineInstance {
    let mut engine = engine();
    engine.map(0x3000, 1, 7).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x3000), &[0x0f, 0x0b])
        .unwrap();
    engine
}
fn put(engine: &mut EngineInstance, at: u32, value: Variant32) {
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(at), &value.0)
        .unwrap();
}
fn value(engine: &EngineInstance, at: u32) -> Variant32 {
    Variant32(read(engine, at, 16).try_into().unwrap())
}
fn text(engine: &mut EngineInstance, at: u32, text: &str) -> u32 {
    let units = text.encode_utf16().collect::<Vec<_>>();
    let bytes = units
        .iter()
        .copied()
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(at), &bytes)
        .unwrap();
    call(engine, Api::SysAllocStringLen, &[at, units.len() as u32])
}
