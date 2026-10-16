mod lookup;
mod strings;
use super::*;
use crate::{
    loader::{Directory32, ParserLimits32},
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};
const BASE: u32 = 0x400000;
fn memory() -> AddressSpace {
    let mut memory = AddressSpace::new(8).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(BASE + 0x1000), 2).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    let mut bytes = [0; 256];
    let half =
        |bytes: &mut [u8], at, value: u16| bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
    let word =
        |bytes: &mut [u8], at, value: u32| bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    half(&mut bytes, 14, 2);
    word(&mut bytes, 16, 6);
    word(&mut bytes, 20, 0x80000030);
    word(&mut bytes, 24, 10);
    word(&mut bytes, 28, 0x80000050);
    for (at, id, child) in [(0x30, 1, 0x70), (0x50, 7, 0x90)] {
        half(&mut bytes, at + 14, 1);
        word(&mut bytes, at + 16, id);
        word(&mut bytes, at + 20, 0x80000000 | child);
    }
    for (at, data) in [(0x70, 0xb0), (0x90, 0xc0)] {
        half(&mut bytes, at + 14, 1);
        word(&mut bytes, at + 16, 0x409);
        word(&mut bytes, at + 20, data);
    }
    word(&mut bytes, 0xb0, 0x2000);
    word(&mut bytes, 0xb4, 42);
    word(&mut bytes, 0xb8, 1200);
    word(&mut bytes, 0xc0, 0x2100);
    word(&mut bytes, 0xc4, 3);
    memory.write(GuestAddress(BASE + 0x1000), &bytes).unwrap();
    let mut strings = [0; 42];
    strings[..2].copy_from_slice(&5u16.to_le_bytes());
    for (index, unit) in "hello".encode_utf16().enumerate() {
        strings[2 + index * 2..4 + index * 2].copy_from_slice(&unit.to_le_bytes());
    }
    memory.write(GuestAddress(BASE + 0x2000), &strings).unwrap();
    memory
        .write(GuestAddress(BASE + 0x2100), &[0xaa, 0xbb, 0xcc])
        .unwrap();
    memory
}
fn reader(memory: &AddressSpace) -> PeResources32<'_> {
    PeResources32::new(
        memory,
        BASE,
        0x3000,
        Directory32 {
            rva: 0x1000,
            size: 256,
        },
        ParserLimits32::default(),
    )
    .unwrap()
}
