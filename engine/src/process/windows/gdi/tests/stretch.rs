use super::{call, fixture, memory};
use crate::{
    memory::{GuestAddress, PageRange, Permissions},
    windows::gdi::GdiApi32,
};
fn surface(
    runtime: &mut super::GdiRuntime32,
    ram: &mut crate::memory::AddressSpace,
    width: u32,
    height: u32,
    bits: u32,
    pixels: &[u8],
) -> (u32, u32) {
    let mut header = vec![
        40,
        width,
        0u32.wrapping_sub(height),
        1 | bits << 16,
        0,
        pixels.len() as u32,
        0,
        0,
        0,
        0,
    ];
    if bits == 1 {
        header.extend([0, 0xffffff]);
    }
    memory::write_words(ram, 0x10000, &header).unwrap();
    let bitmap = call(
        runtime,
        ram,
        GdiApi32::CreateDIBSection,
        &[0, 0x10000, 0, 0x10080, 0, 0],
    )
    .unwrap();
    let [pointer] = memory::words(ram, 0x10080).unwrap();
    ram.write(GuestAddress(pointer), pixels).unwrap();
    let dc = call(runtime, ram, GdiApi32::CreateCompatibleDC, &[0]).unwrap();
    call(runtime, ram, GdiApi32::SelectObject, &[dc, bitmap]).unwrap();
    (dc, pointer)
}
#[test]
fn monochrome_shrink_preserves_native_and_or_and_deleted_scan_samples() {
    for (mode, expected) in [(1, 0x1a), (2, 0xda), (3, 0xda)] {
        let (mut runtime, mut ram) = fixture();
        let (source, _) = surface(&mut runtime, &mut ram, 4, 1, 1, &[0x50, 0, 0, 0]);
        let (target, pointer) = surface(&mut runtime, &mut ram, 4, 1, 1, &[0x5a; 4]);
        assert_eq!(
            call(
                &mut runtime,
                &mut ram,
                GdiApi32::SetStretchBltMode,
                &[target, mode]
            ),
            Ok(1)
        );
        assert_eq!(
            call(
                &mut runtime,
                &mut ram,
                GdiApi32::StretchBlt,
                &[target, 0, 0, 2, 1, source, 0, 0, 4, 1, 0xcc0020]
            ),
            Ok(1)
        );
        let mut bytes = [0; 4];
        ram.read(GuestAddress(pointer), &mut bytes).unwrap();
        assert_eq!(bytes, [expected, 0x5a, 0x5a, 0x5a], "mode {mode}");
    }
}
#[test]
fn halftone_averages_real_source_blocks_and_applies_clip_after_stretching() {
    let (mut runtime, mut ram) = fixture();
    let mut pixels = Vec::new();
    for y in 0..4 {
        for x in 0..4 {
            pixels.extend_from_slice(&if (x + y) % 2 == 0 {
                [0, 0, 0, 0]
            } else {
                [255, 255, 255, 0]
            });
        }
    }
    let (source, _) = surface(&mut runtime, &mut ram, 4, 4, 32, &pixels);
    let (target, pointer) = surface(&mut runtime, &mut ram, 2, 2, 32, &[0x5a; 16]);
    assert_eq!(
        call(
            &mut runtime,
            &mut ram,
            GdiApi32::SetStretchBltMode,
            &[target, 4]
        ),
        Ok(1)
    );
    call(
        &mut runtime,
        &mut ram,
        GdiApi32::IntersectClipRect,
        &[target, 0, 0, 1, 1],
    )
    .unwrap();
    call(
        &mut runtime,
        &mut ram,
        GdiApi32::StretchBlt,
        &[target, 0, 0, 2, 2, source, 0, 0, 4, 4, 0xcc0020],
    )
    .unwrap();
    let mut bytes = [0; 16];
    ram.read(GuestAddress(pointer), &mut bytes).unwrap();
    assert_eq!(&bytes[..4], &[128, 128, 128, 0]);
    assert_eq!(&bytes[4..], &[0x5a; 12]);
    assert_eq!(
        call(
            &mut runtime,
            &mut ram,
            GdiApi32::GetStretchBltMode,
            &[target]
        ),
        Ok(4)
    );
}
#[test]
fn halftone_mono_output_uses_real_ordered_pixels_and_canonical_brush_origin() {
    let (mut runtime, mut ram) = fixture();
    let (source, _) = surface(
        &mut runtime,
        &mut ram,
        4,
        4,
        32,
        &[128, 128, 128, 0].repeat(16),
    );
    let (target, pointer) = surface(&mut runtime, &mut ram, 4, 4, 1, &[0; 16]);
    call(
        &mut runtime,
        &mut ram,
        GdiApi32::SetStretchBltMode,
        &[target, 4],
    )
    .unwrap();
    call(
        &mut runtime,
        &mut ram,
        GdiApi32::SetBrushOrgEx,
        &[target, 0, 0, 0],
    )
    .unwrap();
    call(
        &mut runtime,
        &mut ram,
        GdiApi32::StretchBlt,
        &[target, 0, 0, 4, 4, source, 0, 0, 4, 4, 0xcc0020],
    )
    .unwrap();
    let mut first = [0; 16];
    ram.read(GuestAddress(pointer), &mut first).unwrap();
    assert_eq!(first.iter().map(|byte| byte.count_ones()).sum::<u32>(), 8);
    call(
        &mut runtime,
        &mut ram,
        GdiApi32::SetBrushOrgEx,
        &[target, 1, 0, 0],
    )
    .unwrap();
    call(
        &mut runtime,
        &mut ram,
        GdiApi32::StretchBlt,
        &[target, 0, 0, 4, 4, source, 0, 0, 4, 4, 0xcc0020],
    )
    .unwrap();
    let mut second = [0; 16];
    ram.read(GuestAddress(pointer), &mut second).unwrap();
    assert_ne!(first, second);
    assert_eq!(second.iter().map(|byte| byte.count_ones()).sum::<u32>(), 8);
}
#[test]
fn halftone_fault_preserves_destination_pixels_revision_and_mode_until_rights_repair() {
    let (mut runtime, mut ram) = fixture();
    let (source, _) = surface(&mut runtime, &mut ram, 4, 4, 32, &[255, 0, 0, 0].repeat(16));
    let (target, pointer) = surface(&mut runtime, &mut ram, 2, 2, 32, &[0x5a; 16]);
    call(
        &mut runtime,
        &mut ram,
        GdiApi32::SetStretchBltMode,
        &[target, 4],
    )
    .unwrap();
    let range = PageRange::new(GuestAddress(pointer), 1).unwrap();
    ram.protect(range, Permissions::READ).unwrap();
    let index = runtime.dc_surface(target).unwrap();
    let revision = runtime.surfaces[index].revision;
    let version = ram.transaction_version();
    assert!(
        call(
            &mut runtime,
            &mut ram,
            GdiApi32::StretchBlt,
            &[target, 0, 0, 2, 2, source, 0, 0, 4, 4, 0xcc0020]
        )
        .is_err()
    );
    let mut bytes = [0; 16];
    ram.read(GuestAddress(pointer), &mut bytes).unwrap();
    assert_eq!(bytes, [0x5a; 16]);
    assert_eq!(ram.transaction_version(), version);
    assert_eq!(runtime.surfaces[index].revision, revision);
    assert_eq!(
        call(
            &mut runtime,
            &mut ram,
            GdiApi32::GetStretchBltMode,
            &[target]
        ),
        Ok(4)
    );
    ram.protect(range, Permissions::READ_WRITE).unwrap();
    call(
        &mut runtime,
        &mut ram,
        GdiApi32::StretchBlt,
        &[target, 0, 0, 2, 2, source, 0, 0, 4, 4, 0xcc0020],
    )
    .unwrap();
    ram.read(GuestAddress(pointer), &mut bytes).unwrap();
    assert_eq!(bytes, [255, 0, 0, 0].repeat(4).as_slice());
}
