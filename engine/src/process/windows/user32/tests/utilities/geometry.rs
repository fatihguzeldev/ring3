use super::*;
#[test]
fn rectangle_edges_empty_intersections_and_aliasing_follow_win32_rules() {
    let (mut ctx, mut m, c) = fixture();
    assert_eq!(
        call(
            &mut ctx,
            &mut m,
            c,
            User32Api32::SetRect,
            &[0x10000, 10, 20, 30, 40]
        ),
        1
    );
    assert_eq!(
        call(
            &mut ctx,
            &mut m,
            c,
            User32Api32::PtInRect,
            &[0x10000, 10, 20]
        ),
        1
    );
    assert_eq!(
        call(
            &mut ctx,
            &mut m,
            c,
            User32Api32::PtInRect,
            &[0x10000, 30, 20]
        ),
        0
    );
    call(
        &mut ctx,
        &mut m,
        c,
        User32Api32::SetRect,
        &[0x10020, 30, 20, 50, 40],
    );
    assert_eq!(
        call(
            &mut ctx,
            &mut m,
            c,
            User32Api32::IntersectRect,
            &[0x10000, 0x10000, 0x10020]
        ),
        0
    );
    assert_eq!(
        super::super::super::memory::rect(&m, 0x10000).unwrap(),
        crate::windows::user32::Rect32::default()
    );
    assert_eq!(
        call(
            &mut ctx,
            &mut m,
            c,
            User32Api32::UnionRect,
            &[0x10000, 0x10000, 0x10020]
        ),
        1
    );
    assert_eq!(
        super::super::super::memory::rect(&m, 0x10000).unwrap().left,
        30
    );
}
