use super::{call, fixture, memory};
use crate::{
    memory::GuestAddress,
    windows::gdi::{GdiApi32 as Api, GdiError32},
};
fn target(gdi: &mut super::GdiRuntime32, ram: &mut crate::memory::AddressSpace) -> u32 {
    let dc = call(gdi, ram, Api::CreateCompatibleDC, &[0]).unwrap();
    let bitmap = call(gdi, ram, Api::CreateBitmap, &[4, 4, 1, 32, 0]).unwrap();
    call(gdi, ram, Api::SelectObject, &[dc, bitmap]).unwrap();
    dc
}
fn rgb(ram: &mut crate::memory::AddressSpace, address: u32) {
    memory::write_words(
        ram,
        address,
        &[40, 2, (-2i32) as u32, 1 | 24 << 16, 0, 16, 0, 0, 0, 0],
    )
    .unwrap();
    ram.write(
        GuestAddress(address + 40),
        &[0, 0, 255, 255, 0, 0, 0, 0, 0, 255, 0, 255, 255, 255, 0, 0],
    )
    .unwrap();
}
#[test]
fn packed_rgb_brush_copies_source_and_extended_pen_uses_same_immutable_pattern_owner() {
    let (mut gdi, mut ram) = fixture();
    let dc = target(&mut gdi, &mut ram);
    rgb(&mut ram, 0x10100);
    let brush = call(
        &mut gdi,
        &mut ram,
        Api::CreateDIBPatternBrushPt,
        &[0x10100, 0],
    )
    .unwrap();
    let old = call(&mut gdi, &mut ram, Api::SelectObject, &[dc, brush]).unwrap();
    ram.write(GuestAddress(0x10128), &[0; 16]).unwrap();
    call(
        &mut gdi,
        &mut ram,
        Api::PatBlt,
        &[dc, 0, 0, 4, 4, 0x00f00021],
    )
    .unwrap();
    assert_eq!(
        call(&mut gdi, &mut ram, Api::GetPixel, &[dc, 0, 0]),
        Ok(255)
    );
    assert_eq!(
        call(&mut gdi, &mut ram, Api::GetPixel, &[dc, 1, 0]),
        Ok(0xff0000)
    );
    assert_eq!(
        call(&mut gdi, &mut ram, Api::GetPixel, &[dc, 0, 1]),
        Ok(0x00ff00)
    );
    call(&mut gdi, &mut ram, Api::SelectObject, &[dc, old]).unwrap();
    call(&mut gdi, &mut ram, Api::DeleteObject, &[brush]).unwrap();
    rgb(&mut ram, 0x10100);
    memory::write_words(&mut ram, 0x10200, &[6, 0, 0x10100]).unwrap();
    let pen = call(
        &mut gdi,
        &mut ram,
        Api::ExtCreatePen,
        &[0x10000 | 0x200, 2, 0x10200, 0, 0],
    )
    .unwrap();
    call(&mut gdi, &mut ram, Api::SelectObject, &[dc, pen]).unwrap();
    ram.write(GuestAddress(0x10128), &[0; 16]).unwrap();
    call(&mut gdi, &mut ram, Api::MoveToEx, &[dc, 0, 2, 0]).unwrap();
    call(&mut gdi, &mut ram, Api::LineTo, &[dc, 4, 2]).unwrap();
    assert_eq!(
        call(&mut gdi, &mut ram, Api::GetPixel, &[dc, 0, 2]),
        Ok(255)
    );
}
#[test]
fn packed_palette_brush_binds_each_dc_palette_and_saved_state_independently() {
    let (mut gdi, mut ram) = fixture();
    let a = target(&mut gdi, &mut ram);
    let b = target(&mut gdi, &mut ram);
    memory::write_words(
        &mut ram,
        0x10100,
        &[40, 2, (-1i32) as u32, 1 | 1 << 16, 0, 4, 0, 0, 2, 0],
    )
    .unwrap();
    ram.write(GuestAddress(0x10128), &[1, 0, 0, 0, 0x40, 0, 0, 0])
        .unwrap();
    let brush = call(
        &mut gdi,
        &mut ram,
        Api::CreateDIBPatternBrushPt,
        &[0x10100, 1],
    )
    .unwrap();
    memory::write_words(&mut ram, 0x10200, &[0x00020300, 0x000000ff, 0x00ff0000]).unwrap();
    let pa = call(&mut gdi, &mut ram, Api::CreatePalette, &[0x10200]).unwrap();
    memory::write_words(&mut ram, 0x10200, &[0x00020300, 0x0000ff00, 0x0000ffff]).unwrap();
    let pb = call(&mut gdi, &mut ram, Api::CreatePalette, &[0x10200]).unwrap();
    call(&mut gdi, &mut ram, Api::SelectPalette, &[a, pa, 0]).unwrap();
    call(&mut gdi, &mut ram, Api::SelectPalette, &[b, pb, 0]).unwrap();
    call(&mut gdi, &mut ram, Api::SelectObject, &[a, brush]).unwrap();
    call(&mut gdi, &mut ram, Api::SelectObject, &[b, brush]).unwrap();
    call(&mut gdi, &mut ram, Api::SaveDC, &[a]).unwrap();
    call(&mut gdi, &mut ram, Api::SelectPalette, &[a, pb, 0]).unwrap();
    call(&mut gdi, &mut ram, Api::RestoreDC, &[a, (-1i32) as u32]).unwrap();
    for (dc, first, second) in [(a, 0xff0000, 255), (b, 0xffff, 0xff00)] {
        call(
            &mut gdi,
            &mut ram,
            Api::PatBlt,
            &[dc, 0, 0, 4, 4, 0x00f00021],
        )
        .unwrap();
        assert_eq!(
            call(&mut gdi, &mut ram, Api::GetPixel, &[dc, 0, 0]),
            Ok(first)
        );
        assert_eq!(
            call(&mut gdi, &mut ram, Api::GetPixel, &[dc, 1, 0]),
            Ok(second)
        );
    }
}
#[test]
fn packed_pattern_fault_and_capacity_refuse_without_publishing_object_or_storage() {
    let (mut gdi, mut ram) = fixture();
    rgb(&mut ram, 0x10100);
    let count = gdi.model.object_count();
    let bytes = gdi.bytes;
    let version = ram.transaction_version();
    assert!(matches!(
        call(
            &mut gdi,
            &mut ram,
            Api::CreateDIBPatternBrushPt,
            &[0x12000, 0]
        ),
        Err(GdiError32::Memory(_))
    ));
    assert_eq!(
        call(
            &mut gdi,
            &mut ram,
            Api::CreateDIBPatternBrushPt,
            &[0x10100, 2]
        ),
        Err(GdiError32::InvalidRequest)
    );
    assert_eq!(gdi.model.object_count(), count);
    assert_eq!(gdi.bytes, bytes);
    assert_eq!(ram.transaction_version(), version);
    memory::write_words(
        &mut ram,
        0x10100,
        &[40, 1000000, 2, 1 | 32 << 16, 0, 0, 0, 0, 0, 0],
    )
    .unwrap();
    let version = ram.transaction_version();
    assert_eq!(
        call(
            &mut gdi,
            &mut ram,
            Api::CreateDIBPatternBrushPt,
            &[0x10100, 0]
        ),
        Err(GdiError32::Capacity)
    );
    assert_eq!(gdi.model.object_count(), count);
    assert_eq!(gdi.bytes, bytes);
    assert_eq!(ram.transaction_version(), version);
}
#[test]
fn legacy_global_pattern_uses_real_movable_allocation_and_remains_independent_after_free() {
    use crate::{process::EngineInstance, windows::memory::MemoryApi32};
    let mut engine = EngineInstance::new(128, 7).unwrap();
    engine.map(0x10000, 2, 7).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x10000), &[0x0f, 0x0b])
        .unwrap();
    engine.protect(0x10000, 1, 5).unwrap();
    engine.protect(0x11000, 1, 3).unwrap();
    engine.gdi = Some(
        super::super::GdiRuntime32::new(super::super::GdiConfig32 {
            bitmap_base: 0x20000,
            bitmap_pages: 8,
            bitmap_bytes: 65536,
            screen_width: 8,
            screen_height: 8,
            ..Default::default()
        })
        .unwrap(),
    );
    let captured = super::captured::call_id;
    let global = captured(&mut engine, MemoryApi32::GlobalAlloc.id(), &[2, 56]);
    let pointer = captured(&mut engine, MemoryApi32::GlobalLock.id(), &[global]);
    assert_ne!(global, pointer);
    rgb(engine.memory.as_mut().unwrap(), pointer);
    let brush = captured(&mut engine, Api::CreateDIBPatternBrush.id(), &[global, 0]);
    assert_ne!(brush, 0);
    assert_eq!(
        captured(&mut engine, MemoryApi32::GlobalFree.id(), &[global]),
        0
    );
    let dc = captured(&mut engine, Api::CreateCompatibleDC.id(), &[0]);
    let bitmap = captured(&mut engine, Api::CreateBitmap.id(), &[4, 4, 1, 32, 0]);
    captured(&mut engine, Api::SelectObject.id(), &[dc, bitmap]);
    captured(&mut engine, Api::SelectObject.id(), &[dc, brush]);
    assert_eq!(
        captured(&mut engine, Api::PatBlt.id(), &[dc, 0, 0, 4, 4, 0x00f00021]),
        1
    );
    assert_eq!(captured(&mut engine, Api::GetPixel.id(), &[dc, 0, 0]), 255);
    assert_eq!(
        captured(&mut engine, Api::CreateDIBPatternBrush.id(), &[global, 0]),
        0
    );
}
