mod publication;
mod resources;
mod sources;
use super::sources as source_provider;
use super::*;
use crate::{
    memory::{GuestAddress, PageRange, Permissions},
    process::{EngineInstance, ModuleOperation32, windows::resources::tests as resource_fixture},
    windows::activation::ActivationApi32 as Api,
};
const INPUT: u32 = 0x30000;
const OUTPUT: u32 = 0x31000;
const CLSID: [u8; 16] = [
    0x78, 0x56, 0x34, 0x12, 0xbc, 0x9a, 0xf0, 0xde, 1, 2, 3, 4, 5, 6, 7, 8,
];
fn image() -> Vec<u8> {
    let manifest=b"<assembly xmlns='urn:schemas-microsoft-com:asm.v1' manifestVersion='1.0'><assemblyIdentity name='Ring3.Activation' type='win32' version='1.2.3.4'/><file name='private.dll'><comClass clsid='{12345678-9abc-def0-0102-030405060708}' threadingModel='Both' progid='Ring3.Native.1'/></file></assembly>";
    let mut bytes = resource_fixture::image(0x900000, true, false);
    let data = &mut bytes[0x400..0x800];
    data.fill(0);
    for (offset, value) in [(14, 1u16), (0x20 + 14, 1), (0x40 + 14, 1)] {
        data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    for (offset, value) in [
        (16, 24u32),
        (20, 0x80000020),
        (0x30, 1),
        (0x34, 0x80000040),
        (0x50, 0x409),
        (0x54, 0x60),
        (0x60, 0x2100),
        (0x64, manifest.len() as u32),
    ] {
        data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    data[0x100..0x100 + manifest.len()].copy_from_slice(manifest);
    bytes
}
fn engine(flags: u32) -> (EngineInstance, u32) {
    let mut engine = resource_fixture::engine(false, 16);
    engine
        .configure_activation(ActivationConfig32::default())
        .unwrap();
    resource_fixture::register(&mut engine, "activation.dll", &image());
    let ModuleOperation32::Loaded { handle } = engine
        .load_resource_only_module("activation.dll", flags)
        .unwrap()
    else {
        panic!("resource module is mapped directly")
    };
    engine
        .memory
        .as_mut()
        .unwrap()
        .map_zeroed(
            PageRange::new(GuestAddress(INPUT), 2).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    let words = [32, 0x88, 0, 0, 0, 1, 0, handle];
    write(
        &mut engine,
        INPUT,
        &words
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>(),
    );
    (engine, handle)
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
fn word(engine: &EngineInstance, address: u32) -> u32 {
    u32::from_le_bytes(read(engine, address))
}
fn create(engine: &mut EngineInstance) -> u32 {
    let specification = source_provider::CreateSource32::read(
        engine.memory().unwrap(),
        INPUT,
        true,
        engine.windows_ansi_code_page(),
        0x409,
    )
    .unwrap();
    let mut work = source_provider::CreateWork32::new(specification);
    let limits = engine.activation.as_ref().unwrap().config;
    assert!(matches!(
        work.poll(engine, limits).unwrap(),
        crate::files::FileIo32::Ready(())
    ));
    assert!(work.leases.is_empty());
    engine.publish_activation_context(&work).unwrap()
}
