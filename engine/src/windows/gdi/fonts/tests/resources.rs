use super::super::{FontDatabase32, LogicalFont32};
use crate::windows::gdi::GdiError32;
#[test]
fn removed_memory_source_stays_charged_until_last_realized_face_retires() {
    let bytes = super::source::private_mono();
    let mut fonts = FontDatabase32::new(1_082_044 + bytes.len()).unwrap();
    let id = fonts.add_memory_font(&bytes).unwrap();
    let descriptor = LogicalFont32 {
        face: "Test Mono Face".encode_utf16().collect(),
        ..LogicalFont32::stock(13)
    };
    let selected = fonts.resolve(descriptor.clone()).unwrap();
    let before = selected.shape(&[77, 109], false, false).unwrap();
    fonts.remove_memory_font(id).unwrap();
    assert!(fonts.source_is_live(id));
    assert_eq!(
        selected.face_name(),
        "Test Mono Face".encode_utf16().collect::<Vec<_>>()
    );
    assert_ne!(selected, fonts.resolve(descriptor).unwrap());
    let after = selected.shape(&[77, 109], false, false).unwrap();
    assert_eq!(before.advance, after.advance);
    assert!(matches!(
        fonts.add_memory_font(&bytes),
        Err(GdiError32::Capacity)
    ));
    drop(selected);
    assert!(!fonts.source_is_live(id));
    let replacement = fonts.add_memory_font(&bytes).unwrap();
    assert!(replacement > id);
}
#[test]
fn prepared_font_count_and_catalog_visibility_are_neutral_until_accepted_publication() {
    let bytes = super::source::private_mono();
    let mut fonts = FontDatabase32::new(16 << 20).unwrap();
    let before = fonts.enumerable_fonts().unwrap().len();
    let prepared = fonts.prepare_font_source(&bytes, false).unwrap();
    assert_eq!(prepared.count(), 1);
    assert_eq!(fonts.enumerable_fonts().unwrap().len(), before);
    let memory_id = fonts.commit_font_source(prepared);
    assert_eq!(
        fonts.enumerable_fonts().unwrap().len(),
        before,
        "memory fonts are not enumerable"
    );
    let prepared = fonts.prepare_font_source(&bytes, true).unwrap();
    let file_id = fonts.commit_font_source(prepared);
    assert_eq!(fonts.enumerable_fonts().unwrap().len(), before + 1);
    assert_eq!(fonts.source_count(file_id), Ok(1));
    fonts.remove_memory_font(memory_id).unwrap();
    fonts.remove_memory_font(file_id).unwrap();
    assert_eq!(fonts.enumerable_fonts().unwrap().len(), before);
}
#[test]
fn prepared_source_from_another_database_or_old_issuer_never_changes_catalog() {
    let bytes = include_bytes!("../../../../../assets/fonts/NotoSans-Regular.ttf");
    let mut first = FontDatabase32::new(16 << 20).unwrap();
    let second = FontDatabase32::new(16 << 20).unwrap();
    let old = first.prepare_font_source(bytes, false).unwrap();
    assert_eq!(
        second.check_font_source(&old),
        Err(GdiError32::InvalidRequest)
    );
    let accepted = first.prepare_font_source(bytes, false).unwrap();
    first.commit_font_source(accepted);
    assert_eq!(
        first.check_font_source(&old),
        Err(GdiError32::InvalidRequest)
    );
    assert_eq!(first.enumerable_fonts().unwrap().len(), 2);
}
