mod datafiles;
mod enumeration;
mod formatting;
mod lookup;
use super::*;
use crate::{
    memory::{GuestAddress, PageRange},
    process::{EngineInstance, ModuleOperation32, ModuleRuntimeConfig32},
    windows::resources::ResourceApi32 as Api,
};
const BASE: u32 = 0x400000;
pub(in crate::process::windows) fn image(base: u32, dll: bool, named: bool) -> Vec<u8> {
    fn half(bytes: &mut [u8], at: usize, value: u16) {
        bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
    }
    fn word(bytes: &mut [u8], at: usize, value: u32) {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    }
    let mut bytes = vec![0; 0x800];
    bytes[..2].copy_from_slice(b"MZ");
    word(&mut bytes, 0x3c, 0x80);
    bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
    for (at, value) in [
        (0x84, 0x14c),
        (0x86, 2),
        (0x94, 224),
        (0x96, if dll { 0x2102 } else { 0x102 }),
        (0x98, 0x10b),
        (0xdc, 3),
    ] {
        half(&mut bytes, at, value);
    }
    for (at, value) in [
        (16, if dll { 0 } else { 0x1000 }),
        (28, base),
        (32, 4096),
        (36, 512),
        (56, 0x3000),
        (60, 512),
        (72, 0x100000),
        (76, 4096),
        (80, 0x100000),
        (84, 4096),
        (92, 16),
        (112, 0x2000),
        (116, 256),
    ] {
        word(&mut bytes, 0x98 + at, value);
    }
    for (index, rva, raw, size, flags) in [
        (0, 0x1000, 0x200, 0x200, 0x60000020),
        (1, 0x2000, 0x400, 0x400, 0xc0000040),
    ] {
        let at = 0x178 + 40 * index;
        for (offset, value) in [(8, size), (12, rva), (16, size), (20, raw), (36, flags)] {
            word(&mut bytes, at + offset, value);
        }
    }
    bytes[0x200] = 0xc3;
    let data = &mut bytes[0x400..0x800];
    half(data, 12, u16::from(named));
    half(data, 14, 2);
    let entries = if named {
        vec![(0x800000e0, 0x80000050), (6, 0x80000030), (10, 0x80000050)]
    } else {
        vec![(6, 0x80000030), (10, 0x80000050)]
    };
    for (index, (name, target)) in entries.into_iter().enumerate() {
        word(data, 16 + index * 8, name);
        word(data, 20 + index * 8, target);
    }
    for (at, id, child) in [(0x30, 1, 0x70), (0x50, 7, 0x90)] {
        half(data, at + 14, 1);
        word(data, at + 16, id);
        word(data, at + 20, 0x80000000 | child);
    }
    for (at, target) in [(0x70, 0xb0), (0x90, 0xc0)] {
        half(data, at + 14, 1);
        word(data, at + 16, 0x409);
        word(data, at + 20, target);
    }
    word(data, 0xb0, 0x2100);
    word(data, 0xb4, 42);
    word(data, 0xb8, 1200);
    word(data, 0xc0, 0x2180);
    word(data, 0xc4, 4);
    if named {
        data[0xe0..0xea].copy_from_slice(&[4, 0, b'T', 0, b'Y', 0, b'P', 0, b'E', 0]);
    }
    half(data, 0x100, 5);
    for (index, unit) in "hello".encode_utf16().enumerate() {
        half(data, 0x102 + index * 2, unit);
    }
    data[0x180..0x184].copy_from_slice(&[0x11, 0x22, 0x33, 0x44]);
    bytes
}
pub(in crate::process::windows) fn register(engine: &mut EngineInstance, name: &str, bytes: &[u8]) {
    engine.begin_module_input(bytes.len() as u32, name).unwrap();
    for (index, chunk) in bytes.chunks(4096).enumerate() {
        engine
            .append_module_input((index * 4096) as u32, chunk)
            .unwrap();
    }
    engine.commit_module_input().unwrap();
}
pub(in crate::process::windows) fn engine(named: bool, max_handles: u32) -> EngineInstance {
    engine_with_image(image(BASE, false, named), max_handles, Default::default())
}
fn engine_with_image(
    bytes: Vec<u8>,
    max_handles: u32,
    profile: crate::windows::nls::NlsProfile32,
) -> EngineInstance {
    let mut engine = EngineInstance::new(128, 7).unwrap();
    engine.configure_nls(profile).unwrap();
    engine
        .configure_resources(ResourceConfig32 {
            scratch: PageRange::new(GuestAddress(0x16000000), 64).unwrap(),
            max_handles,
            max_callbacks: 8,
            max_enum_bytes: 1024 * 1024,
        })
        .unwrap();
    register(&mut engine, "resources.exe", &bytes);
    assert!(matches!(
        engine.start_module_process("resources.exe", None, ModuleRuntimeConfig32::default()),
        Ok(ModuleOperation32::Loaded { handle: BASE })
    ));
    engine
}
fn value(engine: &mut EngineInstance, api: Api, args: &[u32]) -> Result<u32, ResourceError32> {
    match engine.resource_call(api, args)? {
        super::types::ResourceCallOutcome32::Return {
            value,
            last_error: None,
        } => Ok(value),
        _ => panic!("expected direct resource return"),
    }
}
fn read<const N: usize>(engine: &EngineInstance, address: u32) -> [u8; N] {
    let mut bytes = [0; N];
    engine
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(address), &mut bytes)
        .unwrap();
    bytes
}
