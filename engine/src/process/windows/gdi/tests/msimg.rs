use super::{call, fixture, memory};
use crate::{
    memory::{Access, GuestAddress, PageRange, Permissions},
    process::windows::gdi::{
        GdiRuntime32,
        raster::canvas::bitmap::{CompositeMode32, CompositeRequest32},
    },
    windows::{
        gdi::{GdiApi32, GdiError32},
        user32::Rect32,
    },
};

fn surface(
    runtime: &mut GdiRuntime32,
    ram: &mut crate::memory::AddressSpace,
    pixels: &[u32; 8],
) -> (u32, u32) {
    let dc = call(runtime, ram, GdiApi32::CreateCompatibleDC, &[0]).unwrap();
    memory::write_words(
        ram,
        0x10000,
        &[40, 4, (-2i32) as u32, 1 | 32 << 16, 0, 32, 0, 0, 0, 0],
    )
    .unwrap();
    let bitmap = call(
        runtime,
        ram,
        GdiApi32::CreateDIBSection,
        &[dc, 0x10000, 0, 0x10080, 0, 0],
    )
    .unwrap();
    let [pointer] = memory::words(ram, 0x10080).unwrap();
    call(runtime, ram, GdiApi32::SelectObject, &[dc, bitmap]).unwrap();
    memory::write_words(ram, pointer, pixels).unwrap();
    (dc, pointer)
}
fn raw(ram: &crate::memory::AddressSpace, pointer: u32) -> [u32; 8] {
    memory::words(ram, pointer).unwrap()
}
fn blend(
    runtime: &mut GdiRuntime32,
    ram: &mut crate::memory::AddressSpace,
    target: u32,
    source: u32,
    constant: u32,
    format: u32,
) -> Result<u32, GdiError32> {
    call(
        runtime,
        ram,
        GdiApi32::AlphaBlend,
        &[
            target,
            0,
            0,
            2,
            1,
            source,
            0,
            0,
            4,
            2,
            constant << 16 | format << 24,
        ],
    )
}

#[test]
fn alpha_blend_separates_premultiplied_pixel_opacity_from_constant_alpha() {
    let (mut runtime, mut ram) = fixture();
    let (source, _) = surface(&mut runtime, &mut ram, &[0x80400000; 8]);
    let (target, pointer) = surface(&mut runtime, &mut ram, &[0x40000040; 8]);
    // authored native input/output, run 39055743538, de910e50, msimg.alpha[0..4].
    for (constant, format, expected) in [
        (255, 1, 0xa0400020),
        (128, 1, 0x70200030),
        (128, 0, 0x60200020),
        (255, 0, 0x80400000),
    ] {
        memory::write_words(&mut ram, pointer, &[0x40000040; 8]).unwrap();
        assert_eq!(
            blend(&mut runtime, &mut ram, target, source, constant, format),
            Ok(1)
        );
        assert_eq!(
            raw(&ram, pointer),
            [
                expected, expected, 0x40000040, 0x40000040, 0x40000040, 0x40000040, 0x40000040,
                0x40000040
            ]
        );
    }
}

#[test]
fn transparent_key_uses_rgb_and_preserves_exact_keyed_destination_and_clip() {
    let (mut runtime, mut ram) = fixture();
    let (source, _) = surface(
        &mut runtime,
        &mut ram,
        &[0x80400000, 0x9900ff00, 0x9900ff00, 0, 0, 0, 0, 0],
    );
    let (target, pointer) = surface(&mut runtime, &mut ram, &[0x40000040; 8]);
    assert_eq!(
        call(
            &mut runtime,
            &mut ram,
            GdiApi32::TransparentBlt,
            &[target, 0, 0, 2, 1, source, 0, 0, 2, 1, 0x40]
        ),
        Ok(1)
    );
    assert_eq!(
        &raw(&ram, pointer)[..3],
        &[0x40000040, 0x0000ff00, 0x40000040]
    );
    call(
        &mut runtime,
        &mut ram,
        GdiApi32::IntersectClipRect,
        &[target, 0, 1, 1, 2],
    )
    .unwrap();
    call(
        &mut runtime,
        &mut ram,
        GdiApi32::TransparentBlt,
        &[target, 0, 1, 2, 1, source, 1, 0, 2, 1, 0x40],
    )
    .unwrap();
    assert_eq!(
        &raw(&ram, pointer)[4..],
        &[0x0000ff00, 0x40000040, 0x40000040, 0x40000040]
    );
}

#[test]
fn gradients_match_authored_rectangle_and_triangle_windows_rows() {
    let (mut runtime, mut ram) = fixture();
    let (target, pointer) = surface(&mut runtime, &mut ram, &[0x5a123456; 8]);
    memory::write_words(
        &mut ram,
        0x10100,
        &[
            0, 0, 0xff00, 0xffff0000, 4, 2, 0, 0xffffff00, 4, 0, 0xff000000, 0xffff0000,
        ],
    )
    .unwrap();
    for (mode, indices, expected) in [
        (
            0,
            [0, 1, 1],
            [
                0xffff0000, 0xffbf003f, 0xff7f007f, 0xff3f00bf, 0xffff0000, 0xffbf003f, 0xff7f007f,
                0xff3f00bf,
            ],
        ),
        (
            1,
            [0, 1, 1],
            [
                0xffff0000, 0xffff0000, 0xffff0000, 0xffff0000, 0xff7f007f, 0xff7f007f, 0xff7f007f,
                0xff7f007f,
            ],
        ),
        (
            2,
            [0, 2, 1],
            [
                0xffff0000, 0xffbf3f00, 0xff7f7f00, 0xff3fbf00, 0x5a123456, 0x5a123456, 0xff7f007f,
                0xff3f3f7f,
            ],
        ),
    ] {
        memory::write_words(&mut ram, pointer, &[0x5a123456; 8]).unwrap();
        memory::write_words(&mut ram, 0x10200, &indices).unwrap();
        assert_eq!(
            call(
                &mut runtime,
                &mut ram,
                GdiApi32::GradientFill,
                &[target, 0x10100, 3, 0x10200, 1, mode]
            ),
            Ok(1)
        );
        assert_eq!(raw(&ram, pointer), expected, "mode {mode}");
    }
}

#[test]
fn invalid_composites_and_late_gradient_indices_leave_pixels_revision_and_ram_unchanged() {
    let (mut runtime, mut ram) = fixture();
    let (source, _) = surface(&mut runtime, &mut ram, &[0x80400000; 8]);
    let (target, pointer) = surface(&mut runtime, &mut ram, &[0x40000040; 8]);
    memory::write_words(
        &mut ram,
        0x10100,
        &[0, 0, 0xff00, 0xffff0000, 4, 2, 0, 0xffffff00],
    )
    .unwrap();
    memory::write_words(&mut ram, 0x10200, &[0, 1, 0, 2]).unwrap();
    let index = runtime.dc_surface(target).unwrap();
    let revision = runtime.surfaces[index].revision;
    let version = ram.transaction_version();
    let requests: &[(GdiApi32, &[u32])] = &[
        (
            GdiApi32::AlphaBlend,
            &[target, 0, 0, 2, 1, source, 0, 0, 4, 2, 0x01ff0001],
        ),
        (
            GdiApi32::AlphaBlend,
            &[target, 0, 0, 2, 1, source, 1, 0, 4, 2, 0x01ff0000],
        ),
        (
            GdiApi32::AlphaBlend,
            &[target, 0, 0, 2, 1, target, 0, 0, 2, 1, 0x01ff0000],
        ),
        (
            GdiApi32::TransparentBlt,
            &[target, 0, 0, u32::MAX, 1, source, 0, 0, 4, 2, 0],
        ),
        (GdiApi32::GradientFill, &[target, 0x10100, 2, 0x10200, 2, 0]),
        (
            GdiApi32::GradientFill,
            &[target, 0x10100, u32::MAX, 0x10200, 1, 0],
        ),
    ];
    for (api, args) in requests {
        assert!(call(&mut runtime, &mut ram, *api, args).is_err());
        assert_eq!(raw(&ram, pointer), [0x40000040; 8]);
        assert_eq!(ram.transaction_version(), version);
        assert_eq!(runtime.surfaces[index].revision, revision);
    }
}

#[test]
fn destination_write_fault_can_retry_without_losing_source_or_publishing_damage() {
    let (mut runtime, mut ram) = fixture();
    let (source, _) = surface(&mut runtime, &mut ram, &[0x80400000; 8]);
    let (target, pointer) = surface(&mut runtime, &mut ram, &[0x40000040; 8]);
    let range = PageRange::new(GuestAddress(pointer), 1).unwrap();
    ram.protect(range, Permissions::READ).unwrap();
    let index = runtime.dc_surface(target).unwrap();
    let revision = runtime.surfaces[index].revision;
    let version = ram.transaction_version();
    let expected = ram
        .check_span(GuestAddress(pointer), 32, Access::Write)
        .unwrap_err();
    assert_eq!(
        blend(&mut runtime, &mut ram, target, source, 255, 1),
        Err(GdiError32::Memory(expected))
    );
    assert_eq!(raw(&ram, pointer), [0x40000040; 8]);
    assert_eq!(ram.transaction_version(), version);
    assert_eq!(runtime.surfaces[index].revision, revision);
    ram.protect(range, Permissions::READ_WRITE).unwrap();
    assert_eq!(blend(&mut runtime, &mut ram, target, source, 255, 1), Ok(1));
    assert_eq!(raw(&ram, pointer)[0], 0xa0400020);
    ram.exhaust_versions_for_test();
    assert_eq!(
        blend(&mut runtime, &mut ram, target, source, 128, 1),
        Err(GdiError32::Memory(
            crate::memory::MemoryError::VersionExhausted
        ))
    );
    assert_eq!(raw(&ram, pointer)[0], 0xa0400020);
}

#[test]
fn canvas_source_revision_is_checked_even_when_owned_pixels_do_not_mutate_guest_ram() {
    let (mut runtime, mut ram) = fixture();
    let source = call(&mut runtime, &mut ram, GdiApi32::CreateCompatibleDC, &[0]).unwrap();
    let bitmap = call(
        &mut runtime,
        &mut ram,
        GdiApi32::CreateCompatibleBitmap,
        &[source, 4, 2],
    )
    .unwrap();
    call(
        &mut runtime,
        &mut ram,
        GdiApi32::SelectObject,
        &[source, bitmap],
    )
    .unwrap();
    let (target, pointer) = surface(&mut runtime, &mut ram, &[0x40000040; 8]);
    let mut canvas = runtime.begin_canvas(&ram, target).unwrap();
    canvas
        .composite(
            &runtime,
            &ram,
            CompositeRequest32 {
                source,
                source_area: Rect32 {
                    left: 0,
                    top: 0,
                    right: 4,
                    bottom: 2,
                },
                destination: Rect32 {
                    left: 0,
                    top: 0,
                    right: 2,
                    bottom: 1,
                },
                mode: CompositeMode32::Transparent(0xffffff),
            },
        )
        .unwrap();
    let version = ram.transaction_version();
    call(
        &mut runtime,
        &mut ram,
        GdiApi32::SetPixel,
        &[source, 0, 0, 0],
    )
    .unwrap();
    assert_eq!(ram.transaction_version(), version);
    assert_eq!(
        runtime.publish_canvas(&mut ram, canvas),
        Err(GdiError32::InvalidRequest)
    );
    assert_eq!(raw(&ram, pointer), [0x40000040; 8]);
}

#[test]
fn msimg_module_exports_use_real_gdi_dispatch_with_exact_stdcall_shapes() {
    use crate::{
        process::EngineInstance,
        windows::gdi::{gdi32_module, msimg32_module},
    };
    let module = msimg32_module(0x72000000);
    assert_eq!(module.name, "msimg32.dll");
    let names: Vec<_> = module
        .exports
        .iter()
        .map(|export| export.name.as_deref().unwrap())
        .collect();
    assert_eq!(names, ["AlphaBlend", "TransparentBlt", "GradientFill"]);
    assert!(
        gdi32_module(0x71000000)
            .exports
            .iter()
            .all(|export| !names.contains(&export.name.as_deref().unwrap()))
    );
    assert_eq!(GdiApi32::AlphaBlend.stack_words(), 11);
    assert_eq!(GdiApi32::TransparentBlt.stack_words(), 11);
    assert_eq!(GdiApi32::GradientFill.stack_words(), 6);
    let mut engine = EngineInstance::new(32, 7).unwrap();
    engine.map(0x10000, 2, 7).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x10000), &[0x0f, 0x0b])
        .unwrap();
    engine.protect(0x10000, 1, 5).unwrap();
    engine.protect(0x11000, 1, 3).unwrap();
    engine.gdi = Some(fixture().0);
    let dc = super::captured::call_id(&mut engine, GdiApi32::CreateCompatibleDC.id(), &[0]);
    for (api, args) in [
        (
            GdiApi32::AlphaBlend,
            vec![dc, 0, 0, 1, 1, dc, 0, 0, 1, 1, 0x01ff0000],
        ),
        (
            GdiApi32::TransparentBlt,
            vec![dc, 0, 0, 1, 1, dc, 0, 0, 1, 1, 0],
        ),
        (GdiApi32::GradientFill, vec![dc, 0, 0, 0, 0, 3]),
    ] {
        let value = super::captured::call_id(&mut engine, api.id(), &args);
        assert_eq!(value, u32::from(api == GdiApi32::TransparentBlt));
    }
}
