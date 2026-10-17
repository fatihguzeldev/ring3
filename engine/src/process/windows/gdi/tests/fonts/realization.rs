use super::super::{call, fixture};
use crate::{
    memory::{AddressSpace, GuestAddress},
    process::windows::gdi::GdiRuntime32,
    windows::gdi::{GdiApi32 as Api, LogicalFont32},
};
fn logical() -> LogicalFont32 {
    LogicalFont32 {
        height: -16,
        face: "Test Font Face".encode_utf16().collect(),
        ..Default::default()
    }
}
fn canvas(runtime: &mut GdiRuntime32, ram: &mut AddressSpace) -> u32 {
    let dc = call(runtime, ram, Api::CreateCompatibleDC, &[0]).unwrap();
    let bitmap = call(runtime, ram, Api::CreateBitmap, &[96, 48, 1, 32, 0]).unwrap();
    call(runtime, ram, Api::SelectObject, &[dc, bitmap]).unwrap();
    call(runtime, ram, Api::SetBkMode, &[dc, 1]).unwrap();
    dc
}
fn draw(runtime: &mut GdiRuntime32, ram: &mut AddressSpace, dc: u32) -> Vec<u8> {
    call(runtime, ram, Api::PatBlt, &[dc, 0, 0, 96, 48, 0x00ff0062]).unwrap();
    let units = [87u16, 109, 87, 105];
    ram.write(
        GuestAddress(0x10100),
        &units
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>(),
    )
    .unwrap();
    call(runtime, ram, Api::TextOutW, &[dc, 2, 2, 0x10100, 4]).unwrap();
    let index = runtime.dc_surface(dc).unwrap();
    runtime.surfaces[index]
        .storage
        .read(ram, runtime.surfaces[index].layout.byte_len().unwrap())
        .unwrap()
}
fn name(runtime: &mut GdiRuntime32, ram: &mut AddressSpace, dc: u32) -> Vec<u16> {
    call(runtime, ram, Api::GetTextFaceW, &[dc, 32, 0x10200]).unwrap();
    let mut bytes = [0; 64];
    ram.read(GuestAddress(0x10200), &mut bytes).unwrap();
    bytes
        .chunks_exact(2)
        .map(|part| u16::from_le_bytes(part.try_into().unwrap()))
        .take_while(|unit| *unit != 0)
        .collect()
}
#[test]
fn removed_source_changes_live_face_query_but_current_physical_glyphs_and_metrics_remain_owned() {
    let (mut runtime, mut ram) = fixture();
    let bytes = crate::windows::gdi::private_font_source_named_for_test("Test Font Face");
    let resource = runtime.fonts.add_memory_font(&bytes).unwrap();
    let font = runtime.create_logical_font(logical()).unwrap();
    let dc = canvas(&mut runtime, &mut ram);
    call(&mut runtime, &mut ram, Api::SelectObject, &[dc, font]).unwrap();
    let expected = "Test Font Face".encode_utf16().collect::<Vec<_>>();
    assert_eq!(name(&mut runtime, &mut ram, dc), expected);
    let before = draw(&mut runtime, &mut ram, dc);
    let advance = runtime
        .measure_text(dc, &[87, 109, 87, 105])
        .unwrap()
        .advance;
    runtime.fonts.remove_memory_font(resource).unwrap();
    assert_ne!(name(&mut runtime, &mut ram, dc), expected);
    assert_eq!(draw(&mut runtime, &mut ram, dc), before);
    let other = canvas(&mut runtime, &mut ram);
    call(&mut runtime, &mut ram, Api::SelectObject, &[other, font]).unwrap();
    assert_ne!(draw(&mut runtime, &mut ram, other), before);
    assert_eq!(
        runtime
            .measure_text(other, &[87, 109, 87, 105])
            .unwrap()
            .advance,
        advance
    );
}
#[test]
fn actual_selection_maps_physical_face_while_equivalent_live_logical_fonts_keep_metric_snapshot() {
    let (mut runtime, mut ram) = fixture();
    let font = runtime.create_logical_font(logical()).unwrap();
    let dc = canvas(&mut runtime, &mut ram);
    call(&mut runtime, &mut ram, Api::SelectObject, &[dc, font]).unwrap();
    let fallback = draw(&mut runtime, &mut ram, dc);
    let metrics = runtime.measure_text(dc, &[87, 109, 87, 105]).unwrap();
    let bytes = crate::windows::gdi::private_font_source_named_for_test("Test Font Face");
    runtime.fonts.add_memory_font(&bytes).unwrap();
    call(&mut runtime, &mut ram, Api::SelectObject, &[dc, font]).unwrap();
    assert_eq!(
        name(&mut runtime, &mut ram, dc),
        "Test Font Face".encode_utf16().collect::<Vec<_>>()
    );
    assert_eq!(draw(&mut runtime, &mut ram, dc), fallback);
    let other = canvas(&mut runtime, &mut ram);
    call(&mut runtime, &mut ram, Api::SelectObject, &[other, font]).unwrap();
    let physical = draw(&mut runtime, &mut ram, other);
    assert_ne!(physical, fallback);
    let second = runtime.create_logical_font(logical()).unwrap();
    call(&mut runtime, &mut ram, Api::SelectObject, &[dc, second]).unwrap();
    assert_eq!(draw(&mut runtime, &mut ram, dc), physical);
    assert_eq!(
        runtime
            .measure_text(dc, &[87, 109, 87, 105])
            .unwrap()
            .advance,
        metrics.advance
    );
    assert_eq!(
        runtime
            .measure_text(other, &[87, 109, 87, 105])
            .unwrap()
            .metrics
            .height,
        metrics.metrics.height
    );
}
