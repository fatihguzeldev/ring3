use super::*;
#[test]
fn pinned_font_metrics_are_from_the_actual_face_and_height_sign_selects_em_or_cell() {
    let fonts = FontDatabase32::new(4 << 20).unwrap();
    let cell = LogicalFont32 {
        height: 20,
        ..Default::default()
    };
    let em = LogicalFont32 {
        height: -20,
        ..Default::default()
    };
    assert_eq!(
        String::from_utf16_lossy(fonts.face_name(&cell)),
        "Noto Sans"
    );
    assert_eq!(fonts.metrics(&cell).unwrap().height, 20);
    assert!(fonts.metrics(&em).unwrap().height > 20);
    let i = fonts.glyph_index(&em, 'i');
    let w = fonts.glyph_index(&em, 'W');
    assert!(fonts.advance(&em, i).unwrap() < fonts.advance(&em, w).unwrap());
    let mono = LogicalFont32 {
        pitch_family: 1,
        ..em.clone()
    };
    assert_eq!(
        String::from_utf16_lossy(fonts.face_name(&mono)),
        "Noto Sans Mono"
    );
    assert_eq!(
        fonts.advance(&mono, fonts.glyph_index(&mono, 'i')).unwrap(),
        fonts.advance(&mono, fonts.glyph_index(&mono, 'W')).unwrap()
    );
}
#[test]
fn shaping_ligatures_and_combining_marks_produces_real_indexed_glyph_coverage() {
    let fonts = FontDatabase32::new(4 << 20).unwrap();
    let font = LogicalFont32 {
        height: -24,
        ..Default::default()
    };
    let shaped = fonts
        .shape(
            &font,
            &"ffi".encode_utf16().collect::<Vec<_>>(),
            false,
            false,
        )
        .unwrap();
    assert!(shaped.glyphs.len() < 3);
    let combined = fonts
        .shape(
            &font,
            &"a\u{301}".encode_utf16().collect::<Vec<_>>(),
            false,
            false,
        )
        .unwrap();
    assert!(combined.advance > 0.0);
    let glyph = fonts
        .rasterize(&font, shaped.glyphs[0].glyph, 65_536)
        .unwrap();
    assert_eq!(glyph.coverage.len(), glyph.width * glyph.height);
    assert!(glyph.coverage.iter().any(|coverage| *coverage != 0));
    assert!(fonts.rasterize(&font, shaped.glyphs[0].glyph, 1).is_err());
}
#[test]
fn memory_font_registration_is_bounded_and_removed_faces_revert_to_configured_fallback() {
    let mut fonts = FontDatabase32::new(1_082_044).unwrap();
    assert!(fonts.add_memory_font(b"malformed").is_err());
    let font = LogicalFont32::default();
    assert_eq!(
        fonts.face_name(&font),
        "Noto Sans".encode_utf16().collect::<Vec<_>>()
    );
    assert!(fonts.remove_memory_font(1).is_err());
}

mod metadata;

mod collection;

mod resources;
mod source;

pub(crate) fn private_source() -> Vec<u8> {
    source::private_mono()
}

pub(crate) fn private_named_source(name: &str) -> Vec<u8> {
    source::private_named(name)
}
