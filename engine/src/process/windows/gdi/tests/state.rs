use super::{call, fixture, memory};
use crate::windows::{
    gdi::{GdiApi32 as Api, GdiError32},
    user32::Point32,
};

#[test]
fn scalar_modes_save_restore_and_invalid_inputs_preserve_dc_state() {
    let (mut runtime, mut ram) = fixture();
    let dc = call(&mut runtime, &mut ram, Api::CreateCompatibleDC, &[0]).unwrap();
    let saved = call(&mut runtime, &mut ram, Api::SaveDC, &[dc]).unwrap();
    for api in [Api::SetPolyFillMode, Api::SetArcDirection] {
        assert_eq!(call(&mut runtime, &mut ram, api, &[dc, 2]), Ok(1));
        assert_eq!(
            call(&mut runtime, &mut ram, api, &[dc, 0]),
            Err(GdiError32::InvalidRequest)
        );
        assert_eq!(call(&mut runtime, &mut ram, api, &[dc, 1]), Ok(2));
    }
    assert_eq!(
        call(&mut runtime, &mut ram, Api::SetMapperFlags, &[dc, 1]),
        Err(GdiError32::Unsupported)
    );
    assert_eq!(
        call(&mut runtime, &mut ram, Api::SetMapperFlags, &[dc, 0]),
        Ok(0)
    );
    assert_eq!(
        call(&mut runtime, &mut ram, Api::SetMapperFlags, &[dc, 2]),
        Err(GdiError32::InvalidRequest)
    );
    assert_eq!(Api::SetMapperFlags.failure_value(), u32::MAX);
    call(&mut runtime, &mut ram, Api::SetPolyFillMode, &[dc, 2]).unwrap();
    call(&mut runtime, &mut ram, Api::RestoreDC, &[dc, saved]).unwrap();
    assert_eq!(
        call(&mut runtime, &mut ram, Api::GetPolyFillMode, &[dc]),
        Ok(1)
    );
    assert_eq!(runtime.model.state(dc).unwrap().mapper_flags, 0);
    assert_eq!(
        call(&mut runtime, &mut ram, Api::SetArcDirection, &[0, 1]),
        Err(GdiError32::InvalidHandle)
    );
}

#[test]
fn extent_scaling_truncates_clamps_and_validates_output_before_state_publication() {
    let (mut runtime, mut ram) = fixture();
    let dc = call(&mut runtime, &mut ram, Api::CreateCompatibleDC, &[0]).unwrap();
    call(&mut runtime, &mut ram, Api::SetMapMode, &[dc, 8]).unwrap();
    call(&mut runtime, &mut ram, Api::SetWindowExtEx, &[dc, 9, 5, 0]).unwrap();
    call(
        &mut runtime,
        &mut ram,
        Api::ScaleWindowExtEx,
        &[dc, 1, 2, (-1i32) as u32, 2, 0x10000],
    )
    .unwrap();
    assert_eq!(memory::words::<2>(&ram, 0x10000).unwrap(), [9, 5]);
    assert_eq!(
        runtime.model.state(dc).unwrap().window_extent,
        Point32 { x: 4, y: -2 }
    );
    let before = runtime.model.state(dc).unwrap().clone();
    assert!(
        call(
            &mut runtime,
            &mut ram,
            Api::ScaleWindowExtEx,
            &[dc, 2, 1, 2, 1, 0x12000]
        )
        .is_err()
    );
    assert_eq!(runtime.model.state(dc).unwrap(), &before);
    assert_eq!(
        call(
            &mut runtime,
            &mut ram,
            Api::ScaleWindowExtEx,
            &[dc, 1, 0, 1, 1, 0]
        ),
        Err(GdiError32::InvalidRequest)
    );
    assert_eq!(runtime.model.state(dc).unwrap(), &before);
    call(
        &mut runtime,
        &mut ram,
        Api::ScaleViewportExtEx,
        &[dc, 1, 1000, 1, 1000, 0],
    )
    .unwrap();
    assert_eq!(
        runtime.model.state(dc).unwrap().viewport_extent,
        Point32 { x: 1, y: 1 }
    );
    call(&mut runtime, &mut ram, Api::SetMapMode, &[dc, 1]).unwrap();
    assert_eq!(
        call(
            &mut runtime,
            &mut ram,
            Api::ScaleWindowExtEx,
            &[dc, 0, 0, 0, 0, 0]
        ),
        Ok(1)
    );
}

#[test]
fn clip_offset_uses_logical_vector_and_current_position_query_is_read_only() {
    let (mut runtime, mut ram) = fixture();
    let dc = call(&mut runtime, &mut ram, Api::CreateCompatibleDC, &[0]).unwrap();
    let region = call(&mut runtime, &mut ram, Api::CreateRectRgn, &[0, 0, 2, 2]).unwrap();
    call(&mut runtime, &mut ram, Api::SelectClipRgn, &[dc, region]).unwrap();
    call(&mut runtime, &mut ram, Api::SetMapMode, &[dc, 8]).unwrap();
    call(
        &mut runtime,
        &mut ram,
        Api::SetViewportExtEx,
        &[dc, 2, 3, 0],
    )
    .unwrap();
    call(
        &mut runtime,
        &mut ram,
        Api::SetViewportOrgEx,
        &[dc, 100, 200, 0],
    )
    .unwrap();
    assert_eq!(
        call(&mut runtime, &mut ram, Api::OffsetClipRgn, &[dc, 4, 5]),
        Ok(2)
    );
    let clip = runtime.model.state(dc).unwrap().clip.as_ref().unwrap();
    assert_eq!(clip.bounds().left, 8);
    assert_eq!(clip.bounds().top, 15);
    call(
        &mut runtime,
        &mut ram,
        Api::MoveToEx,
        &[dc, 7, (-9i32) as u32, 0],
    )
    .unwrap();
    call(
        &mut runtime,
        &mut ram,
        Api::GetCurrentPositionEx,
        &[dc, 0x10000],
    )
    .unwrap();
    assert_eq!(
        memory::words::<2>(&ram, 0x10000).unwrap(),
        [7, (-9i32) as u32]
    );
}

#[test]
fn nearest_color_uses_bitmap_format_and_legacy_character_width_is_measured() {
    let (mut runtime, mut ram) = fixture();
    let dc = call(&mut runtime, &mut ram, Api::CreateCompatibleDC, &[0]).unwrap();
    assert_eq!(
        call(
            &mut runtime,
            &mut ram,
            Api::GetNearestColor,
            &[dc, 0xfefefe]
        ),
        Ok(0xffffff)
    );
    assert_eq!(
        call(&mut runtime, &mut ram, Api::GetNearestColor, &[0, 0]),
        Err(GdiError32::InvalidHandle)
    );
    let bitmap = call(&mut runtime, &mut ram, Api::CreateBitmap, &[2, 2, 1, 32, 0]).unwrap();
    call(&mut runtime, &mut ram, Api::SelectObject, &[dc, bitmap]).unwrap();
    assert_eq!(
        call(
            &mut runtime,
            &mut ram,
            Api::GetNearestColor,
            &[dc, 0x123456]
        ),
        Ok(0x123456)
    );
    call(
        &mut runtime,
        &mut ram,
        Api::GetCharWidthA,
        &[dc, 65, 66, 0x10000],
    )
    .unwrap();
    let widths = memory::words::<2>(&ram, 0x10000).unwrap();
    assert!(widths[0] > 0 && widths[1] > 0);
    call(
        &mut runtime,
        &mut ram,
        Api::GetCharWidth32A,
        &[dc, 65, 66, 0x10008],
    )
    .unwrap();
    assert_eq!(memory::words::<2>(&ram, 0x10008).unwrap(), widths);
}
