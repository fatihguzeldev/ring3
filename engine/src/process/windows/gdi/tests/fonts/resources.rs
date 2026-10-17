use super::super::{call, fixture};
use crate::{
    memory::GuestAddress,
    windows::gdi::{GdiApi32, LogicalFont32},
};
fn name(
    runtime: &mut super::super::super::GdiRuntime32,
    ram: &mut crate::memory::AddressSpace,
    dc: u32,
) -> Vec<u16> {
    call(runtime, ram, GdiApi32::GetTextFaceW, &[dc, 32, 0x10100]).unwrap();
    let mut bytes = [0; 64];
    ram.read(GuestAddress(0x10100), &mut bytes).unwrap();
    bytes
        .chunks_exact(2)
        .map(|part| u16::from_le_bytes(part.try_into().unwrap()))
        .take_while(|unit| *unit != 0)
        .collect()
}
#[test]
fn logical_font_created_before_source_admission_maps_at_first_real_selection() {
    let (mut runtime, mut ram) = fixture();
    let descriptor = LogicalFont32 {
        face: "Test Mono Face".encode_utf16().collect(),
        ..Default::default()
    };
    let font = runtime.create_logical_font(descriptor).unwrap();
    let bytes = crate::windows::gdi::private_font_source_for_test();
    let resource = runtime.fonts.add_memory_font(&bytes).unwrap();
    let dc = call(&mut runtime, &mut ram, GdiApi32::CreateCompatibleDC, &[0]).unwrap();
    call(&mut runtime, &mut ram, GdiApi32::SelectObject, &[dc, font]).unwrap();
    assert_eq!(
        name(&mut runtime, &mut ram, dc),
        "Test Mono Face".encode_utf16().collect::<Vec<_>>()
    );
    runtime.fonts.remove_memory_font(resource).unwrap();
    assert_ne!(
        name(&mut runtime, &mut ram, dc),
        "Test Mono Face".encode_utf16().collect::<Vec<_>>()
    );
    assert_eq!(
        runtime.resolved_dc_font(dc).unwrap().face_name(),
        "Test Mono Face".encode_utf16().collect::<Vec<_>>()
    );
}
#[test]
fn removed_source_before_first_selection_does_not_realize_at_createfont() {
    let (mut runtime, mut ram) = fixture();
    let bytes = crate::windows::gdi::private_font_source_for_test();
    let resource = runtime.fonts.add_memory_font(&bytes).unwrap();
    let font = runtime
        .create_logical_font(LogicalFont32 {
            face: "Test Mono Face".encode_utf16().collect(),
            ..Default::default()
        })
        .unwrap();
    runtime.fonts.remove_memory_font(resource).unwrap();
    let dc = call(&mut runtime, &mut ram, GdiApi32::CreateCompatibleDC, &[0]).unwrap();
    call(&mut runtime, &mut ram, GdiApi32::SelectObject, &[dc, font]).unwrap();
    assert_ne!(
        name(&mut runtime, &mut ram, dc),
        "Test Mono Face".encode_utf16().collect::<Vec<_>>()
    );
}
#[test]
fn saved_dc_retains_source_but_restore_remaps_physical_face_and_keeps_metric_snapshot() {
    let (mut runtime, mut ram) = fixture();
    let bytes = crate::windows::gdi::private_font_source_for_test();
    runtime.fonts = crate::windows::gdi::FontDatabase32::new(1_082_044 + bytes.len()).unwrap();
    let resource = runtime.fonts.add_memory_font(&bytes).unwrap();
    let font = runtime
        .create_logical_font(LogicalFont32 {
            face: "Test Mono Face".encode_utf16().collect(),
            ..Default::default()
        })
        .unwrap();
    let dc = call(&mut runtime, &mut ram, GdiApi32::CreateCompatibleDC, &[0]).unwrap();
    let original = call(&mut runtime, &mut ram, GdiApi32::SelectObject, &[dc, font]).unwrap();
    call(&mut runtime, &mut ram, GdiApi32::SaveDC, &[dc]).unwrap();
    call(
        &mut runtime,
        &mut ram,
        GdiApi32::SelectObject,
        &[dc, original],
    )
    .unwrap();
    runtime.fonts.remove_memory_font(resource).unwrap();
    assert!(runtime.fonts.source_is_live(resource));
    call(&mut runtime, &mut ram, GdiApi32::RestoreDC, &[dc, u32::MAX]).unwrap();
    assert_ne!(
        name(&mut runtime, &mut ram, dc),
        "Test Mono Face".encode_utf16().collect::<Vec<_>>()
    );
    assert_eq!(
        runtime.resolved_dc_metrics(dc).unwrap().face_name(),
        "Test Mono Face".encode_utf16().collect::<Vec<_>>()
    );
    call(
        &mut runtime,
        &mut ram,
        GdiApi32::SelectObject,
        &[dc, original],
    )
    .unwrap();
    call(&mut runtime, &mut ram, GdiApi32::DeleteObject, &[font]).unwrap();
    assert!(!runtime.fonts.source_is_live(resource));
    assert!(runtime.fonts.add_memory_font(&bytes).is_ok());
}
