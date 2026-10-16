use super::super::{FontDatabase32, LogicalFont32};
#[test]
fn physical_metrics_use_pinned_os2_default_character_weight_and_design_measurements() {
    let fonts = FontDatabase32::new(4 << 20).unwrap();
    let logical = LogicalFont32 {
        height: -20,
        weight: 500,
        ..Default::default()
    };
    let metrics = fonts.metrics(&logical).unwrap();
    assert_eq!(
        [
            metrics.height,
            metrics.ascent,
            metrics.descent,
            metrics.internal_leading,
            metrics.external_leading,
            metrics.average_width,
            metrics.maximum_width
        ],
        [27, 21, 6, 7, 0, 12, 57]
    );
    assert_eq!(metrics.default, 0);
    assert_eq!(metrics.break_character, 32);
    assert_eq!(metrics.weight, 400);
    assert_eq!(logical.weight, 500);
}
