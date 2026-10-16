mod lifecycle;
mod publication;

use super::*;
use crate::{
    loader::{ModuleCatalog32, ModuleSet32, SystemModule32},
    memory::{AddressSpace, GuestAddress, PAGE_SIZE, PageRange, Permissions},
};
const PEB: u32 = 0x70000000;
pub(in crate::process) fn image() -> Vec<u8> {
    let mut bytes = vec![0; 1024];
    bytes[..2].copy_from_slice(b"MZ");
    bytes[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
    bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
    for (offset, value) in [
        (0x84, 0x14cu16),
        (0x86, 1),
        (0x94, 224),
        (0x96, 0x102),
        (0x98, 0x10b),
        (0xdc, 3),
    ] {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    for (offset, value) in [
        (16, 0x1000u32),
        (28, 0x400000),
        (32, 4096),
        (36, 512),
        (56, 8192),
        (60, 512),
        (72, 0x100000),
        (76, 4096),
        (80, 0x100000),
        (84, 4096),
        (92, 16),
    ] {
        bytes[0x98 + offset..0x9c + offset].copy_from_slice(&value.to_le_bytes());
    }
    for (offset, value) in [
        (8, 512u32),
        (12, 4096),
        (16, 512),
        (20, 512),
        (36, 0x60000020),
    ] {
        bytes[0x178 + offset..0x17c + offset].copy_from_slice(&value.to_le_bytes());
    }
    bytes[512] = 0xc3;
    bytes
}
pub(in crate::process::windows::process) fn fixture() -> (
    ModuleCatalog32,
    ModuleSet32,
    AddressSpace,
    super::super::super::ProcessStartup32,
) {
    let mut catalog = ModuleCatalog32::new();
    catalog.add_guest("main.exe", &image()).unwrap();
    for (name, preferred_base) in [("first.dll", 0x500000), ("second.dll", 0x600000)] {
        catalog
            .add_system(SystemModule32 {
                name: name.into(),
                preferred_base,
                exports: Vec::new(),
            })
            .unwrap();
    }
    let mut set = ModuleSet32::new();
    let mut memory = AddressSpace::new(32).unwrap();
    let mut plan = catalog.plan_load(&set, &memory, "main.exe", None).unwrap();
    let mut bytes = StartupBytes32::new(PEB, PAGE_SIZE as usize);
    let cwd = bytes.wide("C:\\game").unwrap();
    let environment = bytes.put(&[0; 4], 2).unwrap();
    let (entries, initialized) = stage_loader(
        &mut bytes,
        plan.additions(),
        plan.initialization_order(),
        "C:\\game\\main.exe",
        &"W:\\System".encode_utf16().collect::<Vec<_>>(),
    )
    .unwrap();
    let startup = super::super::super::ProcessStartup32 {
        range: PageRange::new(GuestAddress(PEB), 1).unwrap(),
        peb: PEB,
        command_line: [0; 2],
        image_path: "C:\\game\\main.exe".into(),
        bump: bytes.used,
        environment_blocks: Vec::new(),
        current_environment: (environment, 4),
        free_blocks: Vec::new(),
        allocation_revision: 0,
        current_directory: (cwd.0, usize::from(cwd.1) + 2),
        loader_entries: entries,
        loader_initialization: initialized,
    };
    plan.add_page(GuestAddress(PEB), Permissions::READ_WRITE, &bytes.bytes)
        .unwrap();
    plan.install(&mut memory, &mut set).unwrap();
    (catalog, set, memory, startup)
}
pub(in crate::process::windows::process) fn word(memory: &AddressSpace, address: u32) -> u32 {
    let mut bytes = [0; 4];
    memory.read(GuestAddress(address), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}
pub(in crate::process::windows::process) fn name(
    memory: &AddressSpace,
    descriptor: u32,
) -> Vec<u16> {
    let mut length = [0; 2];
    memory.read(GuestAddress(descriptor), &mut length).unwrap();
    let mut bytes = vec![0; usize::from(u16::from_le_bytes(length))];
    memory
        .read(GuestAddress(word(memory, descriptor + 4)), &mut bytes)
        .unwrap();
    bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect()
}
pub(in crate::process::windows::process) fn list(memory: &AddressSpace, offset: u32) -> Vec<u32> {
    let head = PEB + offset;
    let mut at = word(memory, head);
    let mut previous = head;
    let mut entries = Vec::new();
    while at != head {
        assert!(entries.len() < 32);
        assert_eq!(word(memory, at + 4), previous);
        entries.push(at - (offset - 0x20c));
        previous = at;
        at = word(memory, at);
    }
    assert_eq!(word(memory, head + 4), previous);
    entries
}
pub(in crate::process::windows::process) fn load(
    catalog: &ModuleCatalog32,
    set: &mut ModuleSet32,
    memory: &mut AddressSpace,
    startup: &mut super::super::super::ProcessStartup32,
    name: &str,
) -> ModuleId32 {
    let plan = catalog.plan_load(set, memory, name, None).unwrap();
    let prepared = startup
        .prepare_loader_change(
            plan.additions(),
            plan.initialization_order(),
            &[],
            &"W:\\System".encode_utf16().collect::<Vec<_>>(),
        )
        .unwrap();
    let id = plan.root();
    plan.install_with_writes(memory, set, prepared.words())
        .unwrap();
    startup.commit_loader_change(prepared);
    id
}
fn unload(
    set: &mut ModuleSet32,
    memory: &mut AddressSpace,
    startup: &mut super::super::super::ProcessStartup32,
    id: ModuleId32,
) {
    let plan = set.plan_unload(memory, &[id]).unwrap();
    let prepared = startup.prepare_loader_change(&[], &[], &[id], &[]).unwrap();
    plan.unmap_with_writes(memory, set, prepared.words())
        .unwrap();
    startup.commit_loader_change(prepared);
}
