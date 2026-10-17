use super::*;
use crate::{
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    windows::{
        gdi::{GdiApi32, GdiError32, GdiObject32},
        text::CodePage32,
    },
};
fn fixture() -> (GdiRuntime32, AddressSpace) {
    let runtime = GdiRuntime32::new(GdiConfig32 {
        objects: 16,
        dcs: 8,
        saved_states: 4,
        bitmap_bytes: 65_536,
        bitmap_base: 0x20000,
        bitmap_pages: 16,
        screen_width: 8,
        screen_height: 8,
    })
    .unwrap();
    let mut memory = AddressSpace::new(32).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x10000), 2).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    (runtime, memory)
}
fn call(
    runtime: &mut GdiRuntime32,
    memory: &mut AddressSpace,
    api: GdiApi32,
    args: &[u32],
) -> Result<u32, GdiError32> {
    runtime.execute(memory, api, args, 1, CodePage32::Windows1252)
}
fn dib(runtime: &mut GdiRuntime32, memory: &mut AddressSpace, height: i32) -> (u32, u32) {
    memory::write_words(
        memory,
        0x10000,
        &[40, 3, height as u32, 1 | 32 << 16, 0, 24, 0, 0, 0, 0],
    )
    .unwrap();
    let handle = call(
        runtime,
        memory,
        GdiApi32::CreateDIBSection,
        &[0, 0x10000, 0, 0x10080, 0, 0],
    )
    .unwrap();
    let [pointer] = memory::words(memory, 0x10080).unwrap();
    (handle, pointer)
}
#[test]
fn dib_pointer_is_real_guest_memory_and_selection_blocks_delete_until_restore() {
    let (mut runtime, mut memory) = fixture();
    let dc = call(
        &mut runtime,
        &mut memory,
        GdiApi32::CreateCompatibleDC,
        &[0],
    )
    .unwrap();
    let (bitmap, pointer) = dib(&mut runtime, &mut memory, -2);
    let old = call(
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
            &[dc, 1, 0, 0x00332211]
        ),
        Ok(0x00332211)
    );
    let mut bytes = [0; 4];
    memory.read(GuestAddress(pointer + 4), &mut bytes).unwrap();
    assert_eq!(bytes, [0x33, 0x22, 0x11, 0]);
    memory
        .write(GuestAddress(pointer + 12), &[0x66, 0x55, 0x44, 0])
        .unwrap();
    assert_eq!(
        call(&mut runtime, &mut memory, GdiApi32::GetPixel, &[dc, 0, 1]),
        Ok(0x00665544)
    );
    assert_eq!(
        call(&mut runtime, &mut memory, GdiApi32::DeleteObject, &[bitmap]),
        Err(GdiError32::SelectedObject)
    );
    call(
        &mut runtime,
        &mut memory,
        GdiApi32::SelectObject,
        &[dc, old],
    )
    .unwrap();
    call(&mut runtime, &mut memory, GdiApi32::DeleteObject, &[bitmap]).unwrap();
    assert!(memory.read(GuestAddress(pointer), &mut bytes).is_err());
}
#[test]
fn dib_output_fault_and_version_exhaustion_do_not_publish_handles_or_backing() {
    let (mut runtime, mut memory) = fixture();
    memory::write_words(
        &mut memory,
        0x10000,
        &[40, 3, 2, 1 | 24 << 16, 0, 24, 0, 0, 0, 0],
    )
    .unwrap();
    for pointer in [0, 0x12000, u32::MAX - 1] {
        let version = memory.transaction_version();
        let mapped = memory.mapped_pages();
        assert!(
            call(
                &mut runtime,
                &mut memory,
                GdiApi32::CreateDIBSection,
                &[0, 0x10000, 0, pointer, 0, 0]
            )
            .is_err()
        );
        assert_eq!(runtime.model.object_count(), 0);
        assert_eq!(runtime.bytes, 0);
        assert_eq!(memory.mapped_pages(), mapped);
        assert_eq!(memory.transaction_version(), version);
    }
    memory.exhaust_versions_for_test();
    assert_eq!(
        call(
            &mut runtime,
            &mut memory,
            GdiApi32::CreateDIBSection,
            &[0, 0x10000, 0, 0x10080, 0, 0]
        ),
        Err(GdiError32::Memory(
            crate::memory::MemoryError::VersionExhausted
        ))
    );
    assert_eq!(runtime.model.object_count(), 0);
    assert_eq!(memory.mapped_pages(), 2);
}
#[test]
fn stale_dib_delete_cannot_unmap_a_replacement_at_the_same_guest_address() {
    let (mut runtime, mut memory) = fixture();
    let (bitmap, pointer) = dib(&mut runtime, &mut memory, 2);
    let range = PageRange::new(GuestAddress(pointer), 1).unwrap();
    memory.unmap(range).unwrap();
    memory.map_zeroed(range, Permissions::READ_WRITE).unwrap();
    memory.write(GuestAddress(pointer), b"replacement").unwrap();
    let version = memory.transaction_version();
    assert_eq!(
        call(&mut runtime, &mut memory, GdiApi32::DeleteObject, &[bitmap]),
        Err(GdiError32::InvalidRequest)
    );
    assert_eq!(runtime.model.object_count(), 1);
    assert!(matches!(
        runtime.model.object(bitmap),
        Ok(GdiObject32::Bitmap(_))
    ));
    let mut bytes = [0; 11];
    memory.read(GuestAddress(pointer), &mut bytes).unwrap();
    assert_eq!(&bytes, b"replacement");
    assert_eq!(memory.transaction_version(), version);
}
#[test]
fn overlapping_bitblt_snapshots_source_and_exact_clip_preserves_other_pixels() {
    let (mut runtime, mut memory) = fixture();
    let dc = call(
        &mut runtime,
        &mut memory,
        GdiApi32::CreateCompatibleDC,
        &[0],
    )
    .unwrap();
    let (bitmap, _) = dib(&mut runtime, &mut memory, -2);
    call(
        &mut runtime,
        &mut memory,
        GdiApi32::SelectObject,
        &[dc, bitmap],
    )
    .unwrap();
    for (x, color) in [0x11, 0x22, 0x33].into_iter().enumerate() {
        call(
            &mut runtime,
            &mut memory,
            GdiApi32::SetPixel,
            &[dc, x as u32, 0, color],
        )
        .unwrap();
    }
    call(
        &mut runtime,
        &mut memory,
        GdiApi32::BitBlt,
        &[dc, 1, 0, 2, 1, dc, 0, 0, 0x00cc_0020],
    )
    .unwrap();
    assert_eq!(
        call(&mut runtime, &mut memory, GdiApi32::GetPixel, &[dc, 2, 0]),
        Ok(0x22)
    );
    call(
        &mut runtime,
        &mut memory,
        GdiApi32::IntersectClipRect,
        &[dc, 0, 1, 1, 2],
    )
    .unwrap();
    call(
        &mut runtime,
        &mut memory,
        GdiApi32::PatBlt,
        &[dc, 0, 0, 3, 2, 0x00ff_0062],
    )
    .unwrap();
    let index = runtime.dc_surface(dc).unwrap();
    let bytes = runtime.surfaces[index].storage.read(&memory, 24).unwrap();
    assert_eq!(&bytes[..12], &[0, 0, 0x11, 0, 0, 0, 0x11, 0, 0, 0, 0x22, 0]);
    assert_eq!(&bytes[12..16], &[255, 255, 255, 0]);
    assert_eq!(&bytes[16..], &[0; 8]);
}
#[test]
fn display_pixels_damage_and_exact_revision_ack_are_observable_without_a_browser() {
    let (mut runtime, mut memory) = fixture();
    let dc = call(
        &mut runtime,
        &mut memory,
        GdiApi32::CreateDCA,
        &[0x10100, 0, 0, 0],
    );
    assert!(dc.is_err());
    memory.write(GuestAddress(0x10100), b"DISPLAY\0").unwrap();
    let dc = call(
        &mut runtime,
        &mut memory,
        GdiApi32::CreateDCA,
        &[0x10100, 0, 0, 0],
    )
    .unwrap();
    call(
        &mut runtime,
        &mut memory,
        GdiApi32::SetPixel,
        &[dc, 2, 3, 0x00332211],
    )
    .unwrap();
    let frame = runtime.frame(0).unwrap();
    assert_eq!(frame.format, 1);
    assert_eq!(frame.stride, 32);
    assert_eq!(
        frame.damage,
        crate::windows::user32::Rect32 {
            left: 2,
            top: 3,
            right: 3,
            bottom: 4
        }
    );
    assert_eq!(
        runtime
            .read_frame(frame.id, frame.revision, 3 * 32 + 8, 4)
            .unwrap(),
        &[0x33, 0x22, 0x11, 0]
    );
    call(
        &mut runtime,
        &mut memory,
        GdiApi32::SetPixel,
        &[dc, 1, 1, 0x11],
    )
    .unwrap();
    assert_eq!(
        runtime.acknowledge_frame(frame.id, frame.revision),
        Err(GdiError32::InvalidRequest)
    );
    assert!(runtime.frame(0).is_some());
    let frame = runtime.frame(0).unwrap();
    runtime.acknowledge_frame(frame.id, frame.revision).unwrap();
    assert!(runtime.frame(0).is_none());
}

pub(super) mod captured;

mod paint;

mod fonts;
#[test]
fn dib_protection_fault_can_be_repaired_without_losing_backing_ownership() {
    let (mut runtime, mut memory) = fixture();
    let dc = call(
        &mut runtime,
        &mut memory,
        GdiApi32::CreateCompatibleDC,
        &[0],
    )
    .unwrap();
    let (bitmap, pointer) = dib(&mut runtime, &mut memory, -2);
    call(
        &mut runtime,
        &mut memory,
        GdiApi32::SelectObject,
        &[dc, bitmap],
    )
    .unwrap();
    let range = PageRange::new(GuestAddress(pointer), 1).unwrap();
    memory.protect(range, Permissions::READ).unwrap();
    assert!(
        call(
            &mut runtime,
            &mut memory,
            GdiApi32::SetPixel,
            &[dc, 0, 0, 0x00332211]
        )
        .is_err()
    );
    memory.protect(range, Permissions::READ_WRITE).unwrap();
    assert_eq!(
        call(
            &mut runtime,
            &mut memory,
            GdiApi32::SetPixel,
            &[dc, 0, 0, 0x00332211]
        ),
        Ok(0x00332211)
    );
    let mut bytes = [0; 4];
    memory.read(GuestAddress(pointer), &mut bytes).unwrap();
    assert_eq!(bytes, [0x33, 0x22, 0x11, 0]);
}

mod raster;

mod section;

mod controls;

mod colors;

mod caps;

mod bitfields;

mod palette;

mod canvas;

mod images;

mod visibility;

mod stretch;

mod msimg;

mod patterns;

mod dc;

mod pens;
