mod controls;
mod files;
mod lifecycle;
mod rendering;

use crate::{
    memory::{GuestAddress, PageRange},
    process::{
        EngineInstance,
        windows::{
            gdi::{GdiConfig32, GdiRuntime32, captured_gdi_call_for_test},
            resources::tests as resources,
        },
    },
    windows::{
        gdi::GdiApi32,
        user32::{User32Api32 as Api, User32Limits32},
    },
};
const BASE: u32 = 0x400000;
const DATA: u32 = 0x11000;
pub(in crate::process::windows::user32) fn word(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn half(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}
fn dib(alpha: bool, cursor: bool) -> Vec<u8> {
    let mut bytes = vec![0; 64 + usize::from(cursor) * 4];
    let start = usize::from(cursor) * 4;
    if cursor {
        bytes[..4].copy_from_slice(&[1, 0, 0, 0]);
    }
    for (at, value) in [(0, 40), (4, 2), (8, 4), (12, 1 | 32 << 16)] {
        word(&mut bytes, start + at, value);
    }
    let colors = if alpha {
        [0u32, 0xffffffff, 0x80800000, 0xff00ff00]
    } else {
        [0x000000ff, 0x00ffffff, 0x00ff0000, 0x0000ff00]
    };
    for (index, color) in colors.iter().enumerate() {
        word(&mut bytes, start + 40 + index * 4, *color);
    }
    bytes[start + 56] = 0x40;
    bytes[start + 60] = 0xc0;
    bytes
}
pub(in crate::process::windows::user32) fn resource_image(alpha: bool) -> Vec<u8> {
    let mut image = resources::image(BASE, false, false);
    word(&mut image, 0x98 + 116, 0x400);
    let bytes = &mut image[0x400..];
    bytes.fill(0);
    half(bytes, 14, 5);
    for (index, (kind, id, at, length)) in [
        (1, 101, 0x280, 68),
        (2, 7, 0x200, 56),
        (3, 102, 0x240, 64),
        (12, 5, 0x2f0, 20),
        (14, 6, 0x2d0, 20),
    ]
    .into_iter()
    .enumerate()
    {
        let directory = 0x40 + index * 32;
        let name = 0xe0 + index * 32;
        let entry = 0x180 + index * 16;
        word(bytes, 16 + index * 8, kind);
        word(bytes, 20 + index * 8, 0x80000000 | directory as u32);
        half(bytes, directory + 14, 1);
        word(bytes, directory + 16, id);
        word(bytes, directory + 20, 0x80000000 | name as u32);
        half(bytes, name + 14, 1);
        word(bytes, name + 16, 0x409);
        word(bytes, name + 20, entry as u32);
        word(bytes, entry, 0x2000 + at as u32);
        word(bytes, entry + 4, length);
    }
    let mut bitmap = dib(false, false);
    word(&mut bitmap, 8, (-2i32) as u32);
    bytes[0x200..0x238].copy_from_slice(&bitmap[..56]);
    bytes[0x240..0x280].copy_from_slice(&dib(alpha, false));
    bytes[0x280..0x2c4].copy_from_slice(&dib(false, true));
    for (at, kind, id, length) in [(0x2d0, 1u16, 102u16, 64u32), (0x2f0, 2, 101, 68)] {
        half(bytes, at + 2, kind);
        half(bytes, at + 4, 1);
        if kind == 1 {
            bytes[at + 6] = 2;
            bytes[at + 7] = 2;
        } else {
            half(bytes, at + 6, 2);
            half(bytes, at + 8, 4);
        }
        half(bytes, at + 10, 1);
        half(bytes, at + 12, 32);
        word(bytes, at + 14, length);
        half(bytes, at + 18, id);
    }
    image
}
fn fixture(alpha: bool) -> EngineInstance {
    fixture_image(resource_image(alpha))
}
pub(in crate::process::windows::user32) fn fixture_image(image: Vec<u8>) -> EngineInstance {
    let mut engine = resources::engine_with_image(image, 16, Default::default());
    engine.map(0x10000, 2, 7).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x10000), &[0x0f, 0x0b])
        .unwrap();
    engine.protect(0x10000, 1, 5).unwrap();
    engine.protect(0x11000, 1, 3).unwrap();
    engine.user32 = Some(
        super::super::User32Context32::new(
            User32Limits32 {
                classes: 4,
                windows: 8,
                messages: 8,
                continuations: 4,
            },
            PageRange::new(GuestAddress(0x30000), 8).unwrap(),
        )
        .unwrap(),
    );
    engine.gdi = Some(
        GdiRuntime32::new(GdiConfig32 {
            objects: 16,
            dcs: 4,
            saved_states: 4,
            bitmap_base: 0x20000,
            bitmap_pages: 8,
            bitmap_bytes: 65536,
            screen_width: 8,
            screen_height: 8,
        })
        .unwrap(),
    );
    engine
}
fn call(engine: &mut EngineInstance, api: Api, args: &[u32]) -> u32 {
    captured_gdi_call_for_test(engine, api.id(), args)
}
fn gdi(engine: &mut EngineInstance, api: GdiApi32, args: &[u32]) -> u32 {
    captured_gdi_call_for_test(engine, api.id(), args)
}
fn read_word(engine: &EngineInstance, address: u32) -> u32 {
    let mut bytes = [0; 4];
    engine
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(address), &mut bytes)
        .unwrap();
    u32::from_le_bytes(bytes)
}
