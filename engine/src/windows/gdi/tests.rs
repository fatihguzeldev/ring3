use super::*;
use crate::windows::user32::{Point32, Rect32};
fn context() -> GdiContext32 {
    GdiContext32::new(GdiLimits32 {
        objects: 4,
        dcs: 2,
        saved_states: 3,
    })
    .unwrap()
}
fn dc(context: &mut GdiContext32, thread: u32) -> u32 {
    let prepared = context
        .prepare_dc(DcKind32::Memory {
            owner: Some(thread),
        })
        .unwrap();
    context.commit_new(prepared)
}
#[test]
fn memory_dc_defaults_saved_state_selection_and_exclusive_bitmap_lifetime() {
    let mut context = context();
    let first = dc(&mut context, 1);
    let second = dc(&mut context, 2);
    let default = context.state(first).unwrap().bitmap;
    assert_eq!(
        context.object(default).unwrap(),
        GdiObject32::Bitmap(BitmapLayout32::new(1, 1, 1, false).unwrap())
    );
    let prepared = context
        .prepare_object(GdiObject32::Bitmap(
            BitmapLayout32::new(4, 3, 32, false).unwrap(),
        ))
        .unwrap();
    let bitmap = context.commit_new(prepared);
    assert_eq!(context.select_object(first, bitmap), Ok(default));
    assert_eq!(
        context.select_object(second, bitmap),
        Err(GdiError32::SelectedObject)
    );
    assert_eq!(
        context.delete_object(bitmap),
        Err(GdiError32::SelectedObject)
    );
    assert_eq!(context.save_dc(first), Ok(1));
    context.select_object(first, default).unwrap();
    assert_eq!(
        context.delete_object(bitmap),
        Err(GdiError32::SelectedObject)
    );
    context.restore_dc(first, -1).unwrap();
    assert_eq!(context.state(first).unwrap().bitmap, bitmap);
    assert_eq!(
        context.restore_dc(first, 1),
        Err(GdiError32::InvalidRequest)
    );
    context.select_object(first, default).unwrap();
    context.delete_object(bitmap).unwrap();
    assert_eq!(context.object(bitmap), Err(GdiError32::InvalidHandle));
}
#[test]
fn bounded_dc_reuse_keeps_monotonic_handles_and_creator_thread_ownership() {
    let mut context = context();
    let mut last = 0;
    for thread in 1..40 {
        let handle = dc(&mut context, thread);
        assert!(handle > last);
        last = handle;
        context.retire_thread(thread + 1);
        assert!(context.state(handle).is_ok());
        context.retire_thread(thread);
        assert_eq!(context.state(handle), Err(GdiError32::InvalidHandle));
    }
    let first = dc(&mut context, 91);
    let second = dc(&mut context, 92);
    assert!(matches!(
        context.prepare_dc(DcKind32::Memory { owner: None }),
        Err(GdiError32::Capacity)
    ));
    assert_eq!(context.dc_count(), 2);
    assert!(context.state(first).is_ok());
    assert!(context.state(second).is_ok());
}
#[test]
fn all_ternary_rops_match_literal_truth_table_and_standard_codes() {
    for table in 0..256u32 {
        for bits in 0..8u32 {
            let pattern = if bits & 4 == 0 { 0 } else { u32::MAX };
            let source = if bits & 2 == 0 { 0 } else { u32::MAX };
            let destination = if bits & 1 == 0 { 0 } else { u32::MAX };
            assert_eq!(
                raster_operation32(table << 16, pattern, source, destination),
                if table & (1 << bits) == 0 {
                    0
                } else {
                    u32::MAX
                }
            );
        }
    }
    assert_eq!(
        raster_operation32(0x00cc_0020, 0xaabbcc, 0x123456, 0xfedcba),
        0x123456
    );
    assert_eq!(raster_operation32(0x0055_0009, 0, 0, 0x123456), !0x123456);
    assert!(raster_uses_source32(0x00cc_0020));
    assert!(!raster_uses_source32(0x00f0_0021));
}
#[test]
fn bitmap_dib_rows_stride_colorref_and_top_down_pixels_are_explicit() {
    let bottom = BitmapLayout32::new(3, 2, 24, true).unwrap();
    let top = BitmapLayout32::new(3, -2, 24, true).unwrap();
    assert_eq!(bottom.stride, 12);
    assert_eq!(bottom.byte_len(), Ok(24));
    let mut bytes = vec![0; 24];
    bottom.write_pixel(&mut bytes, &[], 0, 0, 0x00332211);
    assert_eq!(&bytes[12..15], &[0x33, 0x22, 0x11]);
    assert_eq!(bottom.read_pixel(&bytes, &[], 0, 0), 0x00332211);
    assert_eq!(top.read_pixel(&bytes, &[], 0, 1), 0x00332211);
    assert_eq!(BitmapLayout32::new(3, 1, 1, false).unwrap().stride, 2);
    assert_eq!(BitmapLayout32::new(3, 1, 1, true).unwrap().stride, 4);
}
#[test]
fn complex_regions_preserve_holes_offsets_and_disjoint_boolean_results() {
    let outer = Region32::rectangle(Rect32 {
        left: 0,
        top: 0,
        right: 8,
        bottom: 8,
    });
    let inner = Region32::rectangle(Rect32 {
        left: 2,
        top: 2,
        right: 6,
        bottom: 6,
    });
    let ring = outer.combine(&inner, 4).unwrap();
    assert_eq!(ring.complexity(), 3);
    assert!(ring.contains(Point32 { x: 1, y: 3 }));
    assert!(!ring.contains(Point32 { x: 3, y: 3 }));
    assert_eq!(ring.combine(&inner, 2).unwrap(), outer);
    assert!(ring.offset(5, -3).unwrap().contains(Point32 { x: 6, y: 0 }));
    assert_eq!(inner.combine(&ring, 1).unwrap().complexity(), 1);
}
