use super::{call, dib, fixture, memory};
use crate::{
    memory::{GuestAddress, PageRange, Permissions},
    windows::{
        gdi::{DcKind32, GdiApi32, GdiWindow32},
        user32::Point32,
    },
};
#[test]
fn window_dc_resets_attributes_and_releases_even_when_class_owns_client_dc() {
    for class_style in [0x20, 0x40] {
        let (mut runtime, mut memory) = fixture();
        let window = GdiWindow32 {
            window: 9,
            thread: 1,
            class_atom: 0xc000,
            class_style,
            background_brush: 0,
            width: 8,
            height: 8,
            whole: true,
        };
        let first = runtime.acquire_dc(window, 1).unwrap();
        call(
            &mut runtime,
            &mut memory,
            GdiApi32::SetTextColor,
            &[first, 0x123456],
        )
        .unwrap();
        assert_eq!(
            call(&mut runtime, &mut memory, GdiApi32::ReleaseDC, &[9, first]),
            Ok(1)
        );
        assert!(runtime.model.dc_kind(first).is_err());
        let second = runtime.acquire_dc(window, 1).unwrap();
        assert_eq!(runtime.model.state(second).unwrap().text_color, 0);
        assert!(matches!(
            runtime.model.dc_kind(second).unwrap(),
            DcKind32::Display {
                private: false,
                class: None,
                window_dc: true,
                ..
            }
        ));
    }
}
#[test]
fn compatible_bitmap_from_selected_dib_has_distinct_real_backing_and_copied_format() {
    let (mut runtime, mut memory) = fixture();
    let dc = call(
        &mut runtime,
        &mut memory,
        GdiApi32::CreateCompatibleDC,
        &[0],
    )
    .unwrap();
    let (original, old_pointer) = dib(&mut runtime, &mut memory, -2);
    call(
        &mut runtime,
        &mut memory,
        GdiApi32::SelectObject,
        &[dc, original],
    )
    .unwrap();
    memory
        .write(GuestAddress(old_pointer), &[0x33, 0x22, 0x11, 0])
        .unwrap();
    let compatible = call(
        &mut runtime,
        &mut memory,
        GdiApi32::CreateCompatibleBitmap,
        &[dc, 4, 5],
    )
    .unwrap();
    assert_ne!(compatible, original);
    assert_eq!(
        call(
            &mut runtime,
            &mut memory,
            GdiApi32::GetObjectW,
            &[compatible, 84, 0x10100]
        ),
        Ok(84)
    );
    let [_, width, height, stride, format, pointer]: [u32; 6] =
        memory::words(&memory, 0x10100).unwrap();
    assert_eq!([width, height, stride, format], [4, 5, 16, 1 | 32 << 16]);
    assert_ne!(pointer, old_pointer);
    assert_ne!(pointer, 0);
    call(
        &mut runtime,
        &mut memory,
        GdiApi32::DeleteObject,
        &[compatible],
    )
    .unwrap();
    let mut bytes = [0; 4];
    memory.read(GuestAddress(old_pointer), &mut bytes).unwrap();
    assert_eq!(bytes, [0x33, 0x22, 0x11, 0]);
    assert!(memory.read(GuestAddress(pointer), &mut bytes).is_err());
}
#[test]
fn isotropic_mode_adjusts_viewport_and_previous_extent_output_fault_is_neutral() {
    let (mut runtime, mut memory) = fixture();
    let dc = call(
        &mut runtime,
        &mut memory,
        GdiApi32::CreateCompatibleDC,
        &[0],
    )
    .unwrap();
    call(&mut runtime, &mut memory, GdiApi32::SetMapMode, &[dc, 7]).unwrap();
    call(
        &mut runtime,
        &mut memory,
        GdiApi32::SetWindowExtEx,
        &[dc, 100, 200, 0],
    )
    .unwrap();
    let before = runtime.model.state(dc).unwrap().clone();
    assert!(
        call(
            &mut runtime,
            &mut memory,
            GdiApi32::SetViewportExtEx,
            &[dc, 300, 300, 0x12000]
        )
        .is_err()
    );
    assert_eq!(runtime.model.state(dc).unwrap(), &before);
    call(
        &mut runtime,
        &mut memory,
        GdiApi32::SetViewportExtEx,
        &[dc, 300, 300, 0x10100],
    )
    .unwrap();
    let state = runtime.model.state(dc).unwrap();
    assert_eq!(state.viewport_extent, Point32 { x: 150, y: 300 });
    let x = state.device_point(Point32 { x: 10, y: 0 }).unwrap();
    let y = state.device_point(Point32 { x: 0, y: 10 }).unwrap();
    assert_eq!(x.x, y.y);
}
#[test]
#[ignore = "bounded manual GDI pixel throughput measurement"]
fn single_pixel_updates_on_four_megabyte_dib() {
    let mut runtime = super::GdiRuntime32::new(super::GdiConfig32 {
        objects: 16,
        dcs: 4,
        saved_states: 4,
        bitmap_bytes: 8 << 20,
        bitmap_base: 0x200000,
        bitmap_pages: 1024,
        screen_width: 8,
        screen_height: 8,
    })
    .unwrap();
    let mut memory = crate::memory::AddressSpace::new(1030).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x10000), 2).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    memory::write_words(
        &mut memory,
        0x10000,
        &[
            40,
            1024,
            (-1024i32) as u32,
            1 | 32 << 16,
            0,
            4 << 20,
            0,
            0,
            0,
            0,
        ],
    )
    .unwrap();
    let bitmap = call(
        &mut runtime,
        &mut memory,
        GdiApi32::CreateDIBSection,
        &[0, 0x10000, 0, 0x10080, 0, 0],
    )
    .unwrap();
    let dc = call(
        &mut runtime,
        &mut memory,
        GdiApi32::CreateCompatibleDC,
        &[0],
    )
    .unwrap();
    call(
        &mut runtime,
        &mut memory,
        GdiApi32::SelectObject,
        &[dc, bitmap],
    )
    .unwrap();
    let start = std::time::Instant::now();
    for index in 0..1000 {
        assert_eq!(
            call(
                &mut runtime,
                &mut memory,
                GdiApi32::SetPixel,
                &[dc, index % 1024, index * 17 % 1024, 0x00332211]
            ),
            Ok(0x00332211)
        );
    }
    eprintln!("GDI_SETPIXEL_1000_4MIB_NS={}", start.elapsed().as_nanos());
    assert_eq!(
        call(
            &mut runtime,
            &mut memory,
            GdiApi32::GetPixel,
            &[dc, 999, 999 * 17 % 1024]
        ),
        Ok(0x00332211)
    );
}
#[test]
fn packed_pixel_span_preserves_neighbor_bits_and_failed_dib_write_keeps_revision() {
    let (mut runtime, mut memory) = fixture();
    let dc = call(
        &mut runtime,
        &mut memory,
        GdiApi32::CreateCompatibleDC,
        &[0],
    )
    .unwrap();
    let bitmap = call(
        &mut runtime,
        &mut memory,
        GdiApi32::CreateBitmap,
        &[16, 1, 1, 1, 0],
    )
    .unwrap();
    call(
        &mut runtime,
        &mut memory,
        GdiApi32::SelectObject,
        &[dc, bitmap],
    )
    .unwrap();
    assert_eq!(
        call(
            &mut runtime,
            &mut memory,
            GdiApi32::SetPixel,
            &[dc, 9, 0, 0xffffff]
        ),
        Ok(0xffffff)
    );
    call(
        &mut runtime,
        &mut memory,
        GdiApi32::GetBitmapBits,
        &[bitmap, 2, 0x10100],
    )
    .unwrap();
    let mut packed = [0; 2];
    memory.read(GuestAddress(0x10100), &mut packed).unwrap();
    assert_eq!(packed, [0, 0x40]);
    assert_eq!(
        call(&mut runtime, &mut memory, GdiApi32::GetPixel, &[dc, 8, 0]),
        Ok(0)
    );
    let (dib, pointer) = dib(&mut runtime, &mut memory, -2);
    call(
        &mut runtime,
        &mut memory,
        GdiApi32::SelectObject,
        &[dc, dib],
    )
    .unwrap();
    let index = runtime.dc_surface(dc).unwrap();
    let revision = runtime.surfaces[index].revision;
    memory.exhaust_versions_for_test();
    assert!(
        call(
            &mut runtime,
            &mut memory,
            GdiApi32::SetPixel,
            &[dc, 2, 0, 0x332211]
        )
        .is_err()
    );
    assert_eq!(runtime.surfaces[index].revision, revision);
    let mut bytes = [0; 4];
    memory.read(GuestAddress(pointer + 8), &mut bytes).unwrap();
    assert_eq!(bytes, [0; 4]);
}

mod semantics;
