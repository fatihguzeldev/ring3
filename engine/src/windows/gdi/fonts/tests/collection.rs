use super::super::{FontDatabase32, LogicalFont32};
use crate::windows::gdi::GdiError32;

use super::source::{collection, private_mono};

#[test]
fn memory_collection_installs_the_second_real_face_and_removal_retires_the_group() {
    let mono = private_mono();
    let bytes = collection(&[
        include_bytes!("../../../../../assets/fonts/NotoSans-Regular.ttf"),
        &mono,
    ]);
    assert_eq!(rustybuzz::ttf_parser::fonts_in_collection(&bytes), Some(2));
    let parsed = rustybuzz::ttf_parser::Face::parse(&bytes, 1).unwrap();
    let fixed_pitch = parsed.is_monospaced();
    assert_eq!(
        parsed.glyph_hor_advance(parsed.glyph_index('i').unwrap()),
        parsed.glyph_hor_advance(parsed.glyph_index('M').unwrap())
    );
    let mut fonts = FontDatabase32::new(4 << 20).unwrap();
    let resource = fonts.add_memory_font(&bytes).unwrap();
    let selected = LogicalFont32 {
        height: -20,
        face: "Test Mono Face".encode_utf16().collect(),
        ..Default::default()
    };
    assert_eq!(
        String::from_utf16_lossy(fonts.face_name(&selected)),
        "Test Mono Face"
    );
    assert_eq!(fonts.metrics(&selected).unwrap().fixed_pitch, fixed_pitch);
    let glyph = fonts.glyph_index(&selected, 'M');
    let raster = fonts.rasterize(&selected, glyph, 65_536).unwrap();
    assert!(raster.coverage.iter().any(|value| *value != 0));
    fonts.remove_memory_font(resource).unwrap();
    assert_eq!(
        String::from_utf16_lossy(fonts.face_name(&selected)),
        "Noto Sans Mono"
    );
}

#[test]
fn malformed_second_collection_face_does_not_publish_the_valid_first_face() {
    let mono = private_mono();
    let mut bytes = collection(&[
        &mono,
        include_bytes!("../../../../../assets/fonts/NotoSans-Regular.ttf"),
    ]);
    bytes[16..20].copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(rustybuzz::ttf_parser::Face::parse(&bytes, 0).is_ok());
    assert!(rustybuzz::ttf_parser::Face::parse(&bytes, 1).is_err());
    let mut fonts = FontDatabase32::new(4 << 20).unwrap();
    let font = LogicalFont32 {
        face: "Test Mono Face".encode_utf16().collect(),
        ..Default::default()
    };
    assert_eq!(
        fonts.add_memory_font(&bytes),
        Err(GdiError32::InvalidRequest)
    );
    assert_eq!(
        String::from_utf16_lossy(fonts.face_name(&font)),
        "Noto Sans Mono"
    );
    let registered = fonts.add_memory_font(&mono).unwrap();
    assert_eq!(
        String::from_utf16_lossy(fonts.face_name(&font)),
        "Test Mono Face"
    );
    fonts.remove_memory_font(registered).unwrap();
}

#[test]
fn collection_source_bytes_are_charged_once_and_group_retirement_restores_budget() {
    let mono = private_mono();
    let bytes = collection(&[
        include_bytes!("../../../../../assets/fonts/NotoSans-Regular.ttf"),
        &mono,
    ]);
    let mut fonts = FontDatabase32::new(1_082_044 + bytes.len()).unwrap();
    let resource = fonts.add_memory_font(&bytes).unwrap();
    assert_eq!(fonts.add_memory_font(&mono), Err(GdiError32::Capacity));
    fonts.remove_memory_font(resource).unwrap();
    let replacement = fonts.add_memory_font(&bytes).unwrap();
    assert_ne!(replacement, resource);
    fonts.remove_memory_font(replacement).unwrap();
}
