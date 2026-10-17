use super::{call, fixture, memory as guest_memory};
use crate::{
    memory::{GuestAddress, MemoryError},
    windows::gdi::{GdiApi32, GdiError32},
};
fn font(
    runtime: &mut super::GdiRuntime32,
    memory: &mut crate::memory::AddressSpace,
    height: i32,
) -> u32 {
    call(
        runtime,
        memory,
        GdiApi32::CreateFontW,
        &[height as u32, 0, 0, 0, 400, 0, 0, 0, 1, 0, 0, 0, 0, 0],
    )
    .unwrap()
}
fn canvas(
    runtime: &mut super::GdiRuntime32,
    memory: &mut crate::memory::AddressSpace,
) -> (u32, u32) {
    guest_memory::write_words(
        memory,
        0x10000,
        &[40, 64, (-32i32) as u32, 1 | 32 << 16, 0, 8192, 0, 0, 0, 0],
    )
    .unwrap();
    let bitmap = call(
        runtime,
        memory,
        GdiApi32::CreateDIBSection,
        &[0, 0x10000, 0, 0x10080, 0, 0],
    )
    .unwrap();
    let dc = call(runtime, memory, GdiApi32::CreateCompatibleDC, &[0]).unwrap();
    call(runtime, memory, GdiApi32::SelectObject, &[dc, bitmap]).unwrap();
    let [pointer] = guest_memory::words(memory, 0x10080).unwrap();
    (dc, pointer)
}
#[test]
fn logical_font_selection_metrics_face_and_saved_dc_lifetime_use_actual_open_font() {
    let (mut runtime, mut memory) = fixture();
    let dc = call(
        &mut runtime,
        &mut memory,
        GdiApi32::CreateCompatibleDC,
        &[0],
    )
    .unwrap();
    let font = font(&mut runtime, &mut memory, -20);
    let previous = call(
        &mut runtime,
        &mut memory,
        GdiApi32::SelectObject,
        &[dc, font],
    )
    .unwrap();
    call(
        &mut runtime,
        &mut memory,
        GdiApi32::GetTextMetricsW,
        &[dc, 0x10100],
    )
    .unwrap();
    let [height, ascent, descent]: [u32; 3] = guest_memory::words(&memory, 0x10100).unwrap();
    assert_eq!(height, ascent + descent);
    assert!(height > 20);
    let count = call(
        &mut runtime,
        &mut memory,
        GdiApi32::GetTextFaceW,
        &[dc, 0, 0],
    )
    .unwrap();
    assert_eq!(count, 10);
    assert_eq!(
        call(
            &mut runtime,
            &mut memory,
            GdiApi32::GetTextFaceW,
            &[dc, 32, 0x10200]
        ),
        Ok(10)
    );
    let mut bytes = [0; 20];
    memory.read(GuestAddress(0x10200), &mut bytes).unwrap();
    assert_eq!(
        bytes.as_slice(),
        "Noto Sans\0"
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()
    );
    call(&mut runtime, &mut memory, GdiApi32::SaveDC, &[dc]).unwrap();
    call(
        &mut runtime,
        &mut memory,
        GdiApi32::SelectObject,
        &[dc, previous],
    )
    .unwrap();
    assert_eq!(
        call(&mut runtime, &mut memory, GdiApi32::DeleteObject, &[font]),
        Err(GdiError32::SelectedObject)
    );
    call(
        &mut runtime,
        &mut memory,
        GdiApi32::RestoreDC,
        &[dc, (-1i32) as u32],
    )
    .unwrap();
    call(
        &mut runtime,
        &mut memory,
        GdiApi32::SelectObject,
        &[dc, previous],
    )
    .unwrap();
    assert_eq!(
        call(&mut runtime, &mut memory, GdiApi32::DeleteObject, &[font]),
        Ok(1)
    );
}
#[test]
fn textout_rasterizes_real_glyphs_and_exttextout_opaque_clip_changes_only_requested_pixels() {
    let (mut runtime, mut memory) = fixture();
    let (dc, pointer) = canvas(&mut runtime, &mut memory);
    let font = font(&mut runtime, &mut memory, -18);
    call(
        &mut runtime,
        &mut memory,
        GdiApi32::SelectObject,
        &[dc, font],
    )
    .unwrap();
    memory
        .write(
            GuestAddress(0x10100),
            &"Wi"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    assert_eq!(
        call(
            &mut runtime,
            &mut memory,
            GdiApi32::TextOutW,
            &[dc, 2, 1, 0x10100, 2]
        ),
        Ok(1)
    );
    let mut pixels = vec![0; 8192];
    memory.read(GuestAddress(pointer), &mut pixels).unwrap();
    assert!(pixels.chunks_exact(4).any(|pixel| pixel[0] > 0
        && pixel[0] < 255
        && pixel[0] == pixel[1]
        && pixel[1] == pixel[2]));
    assert!(pixels.chunks_exact(4).any(|pixel| pixel[..3] == [255; 3]));
    guest_memory::write_words(&mut memory, 0x10200, &[40, 0, 50, 10]).unwrap();
    call(
        &mut runtime,
        &mut memory,
        GdiApi32::SetBkColor,
        &[dc, 0x00332211],
    )
    .unwrap();
    assert_eq!(
        call(
            &mut runtime,
            &mut memory,
            GdiApi32::ExtTextOutW,
            &[dc, 0, 0, 6, 0x10200, 0, 0, 0]
        ),
        Ok(1)
    );
    let mut after = vec![0; 8192];
    memory.read(GuestAddress(pointer), &mut after).unwrap();
    for y in 0..32 {
        for x in 0..64 {
            let at = (y * 64 + x) * 4;
            if x >= 40 && x < 50 && y < 10 {
                assert_eq!(&after[at..at + 4], &[0x33, 0x22, 0x11, 0]);
            } else {
                assert_eq!(&after[at..at + 4], &pixels[at..at + 4]);
            }
        }
    }
}
#[test]
fn font_query_scatter_fault_does_not_write_earlier_size_fit_or_extent_outputs() {
    let (mut runtime, mut memory) = fixture();
    let dc = call(
        &mut runtime,
        &mut memory,
        GdiApi32::CreateCompatibleDC,
        &[0],
    )
    .unwrap();
    memory.write(GuestAddress(0x10100), b"Wi").unwrap();
    memory.write(GuestAddress(0x10200), &[0xa5; 16]).unwrap();
    let version = memory.transaction_version();
    assert!(
        call(
            &mut runtime,
            &mut memory,
            GdiApi32::GetTextExtentExPointA,
            &[dc, 0x10100, 2, 100, 0x10208, 0x12000, 0x10200]
        )
        .is_err()
    );
    let mut bytes = [0; 16];
    memory.read(GuestAddress(0x10200), &mut bytes).unwrap();
    assert_eq!(bytes, [0xa5; 16]);
    assert_eq!(memory.transaction_version(), version);
}
#[test]
fn text_version_exhaustion_preserves_dib_pixels_dc_position_and_surface_revision() {
    let (mut runtime, mut memory) = fixture();
    let (dc, pointer) = canvas(&mut runtime, &mut memory);
    let font = font(&mut runtime, &mut memory, -18);
    call(
        &mut runtime,
        &mut memory,
        GdiApi32::SelectObject,
        &[dc, font],
    )
    .unwrap();
    call(&mut runtime, &mut memory, GdiApi32::SetTextAlign, &[dc, 1]).unwrap();
    memory.write(GuestAddress(0x10100), b"Wi").unwrap();
    let state = runtime.model.state(dc).unwrap().clone();
    let revision = runtime.surfaces[runtime.dc_surface(dc).unwrap()].revision;
    memory.exhaust_versions_for_test();
    assert_eq!(
        call(
            &mut runtime,
            &mut memory,
            GdiApi32::TextOutA,
            &[dc, 3, 4, 0x10100, 2]
        ),
        Err(GdiError32::Memory(MemoryError::VersionExhausted))
    );
    let mut bytes = vec![0; 8192];
    memory.read(GuestAddress(pointer), &mut bytes).unwrap();
    assert_eq!(bytes, vec![0; 8192]);
    assert_eq!(runtime.model.state(dc).unwrap(), &state);
    assert_eq!(
        runtime.surfaces[runtime.dc_surface(dc).unwrap()].revision,
        revision
    );
}
#[test]
fn drawtext_calcrect_lines_and_tab_stops_use_actual_selected_font_metrics() {
    let (mut runtime, mut memory) = fixture();
    let dc = call(
        &mut runtime,
        &mut memory,
        GdiApi32::CreateCompatibleDC,
        &[0],
    )
    .unwrap();
    let font = font(&mut runtime, &mut memory, -18);
    call(
        &mut runtime,
        &mut memory,
        GdiApi32::SelectObject,
        &[dc, font],
    )
    .unwrap();
    let units = "Wi\nWi".encode_utf16().collect::<Vec<_>>();
    memory
        .write(
            GuestAddress(0x10100),
            &units
                .iter()
                .flat_map(|unit| unit.to_le_bytes())
                .collect::<Vec<_>>(),
        )
        .unwrap();
    guest_memory::write_words(&mut memory, 0x10200, &[0, 0, 64, 32]).unwrap();
    let measured = runtime.measure_text(dc, &units[..2]).unwrap();
    assert_eq!(
        call(
            &mut runtime,
            &mut memory,
            GdiApi32::DrawTextW,
            &[dc, 0x10100, 5, 0x10200, 0x400]
        ),
        Ok(measured.metrics.height as u32 * 2)
    );
    let [left, top, right, bottom]: [u32; 4] = guest_memory::words(&memory, 0x10200).unwrap();
    assert_eq!(
        [left, top, right, bottom],
        [
            0,
            0,
            measured.advance.round() as u32,
            measured.metrics.height as u32 * 2
        ]
    );
    memory
        .write(
            GuestAddress(0x10100),
            &"i\tW"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
    guest_memory::write_words(&mut memory, 0x10200, &[0, 0, 200, 32]).unwrap();
    let w = runtime.measure_text(dc, &[u16::from(b'W')]).unwrap();
    call(
        &mut runtime,
        &mut memory,
        GdiApi32::DrawTextW,
        &[dc, 0x10100, 3, 0x10200, 0x440],
    )
    .unwrap();
    let [_, _, right, _]: [u32; 4] = guest_memory::words(&memory, 0x10200).unwrap();
    assert_eq!(
        right,
        (w.metrics.average_width * 8) as u32 + w.advance.round() as u32
    );
}
#[test]
fn drawtextex_output_fault_is_atomic_with_dib_pixels_and_dc_state() {
    let (mut runtime, mut memory) = fixture();
    let (dc, pointer) = canvas(&mut runtime, &mut memory);
    memory.write(GuestAddress(0x10100), b"Wi").unwrap();
    guest_memory::write_words(&mut memory, 0x10200, &[0, 0, 64, 32]).unwrap();
    guest_memory::write_words(&mut memory, 0x11000, &[20, 8, 0, 0, 0xa5a5a5a5]).unwrap();
    memory
        .protect(
            crate::memory::PageRange::new(GuestAddress(0x11000), 1).unwrap(),
            crate::memory::Permissions::READ,
        )
        .unwrap();
    let version = memory.transaction_version();
    let state = runtime.model.state(dc).unwrap().clone();
    assert!(
        call(
            &mut runtime,
            &mut memory,
            GdiApi32::DrawTextExA,
            &[dc, 0x10100, 2, 0x10200, 0, 0x11000]
        )
        .is_err()
    );
    let mut pixels = vec![0; 8192];
    memory.read(GuestAddress(pointer), &mut pixels).unwrap();
    assert_eq!(pixels, vec![0; 8192]);
    assert_eq!(runtime.model.state(dc).unwrap(), &state);
    assert_eq!(memory.transaction_version(), version);
    assert_eq!(guest_memory::words::<1>(&memory, 0x11010).unwrap(), [0xa5a5a5a5]);
}
#[test]
fn drawtext_prefix_only_emits_visible_mnemonic_underline_without_glyph_or_background() {
    let (mut runtime, mut memory) = fixture();
    let (dc, pointer) = canvas(&mut runtime, &mut memory);
    call(
        &mut runtime,
        &mut memory,
        GdiApi32::SetTextColor,
        &[dc, 0xffffff],
    )
    .unwrap();
    memory.write(GuestAddress(0x10100), b"&Wi").unwrap();
    guest_memory::write_words(&mut memory, 0x10200, &[0, 0, 64, 32]).unwrap();
    call(
        &mut runtime,
        &mut memory,
        GdiApi32::DrawTextA,
        &[dc, 0x10100, 3, 0x10200, 0x200000],
    )
    .unwrap();
    let mut pixels = vec![0; 8192];
    memory.read(GuestAddress(pointer), &mut pixels).unwrap();
    let rows = pixels
        .chunks_exact(256)
        .filter(|row| row.iter().any(|byte| *byte != 0))
        .count();
    assert!(rows > 0 && rows <= 2);
    assert!(
        pixels
            .chunks_exact(4)
            .filter(|pixel| pixel[0] != 0)
            .all(|pixel| pixel == [255, 255, 255, 0])
    );
}

mod rotation;

mod resources;

mod enumeration;
mod memory;

mod files;

mod realization;
