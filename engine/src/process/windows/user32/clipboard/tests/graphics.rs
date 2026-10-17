use super::{KEY, call, fixture};
use crate::{
    memory::{GuestAddress, MemoryError},
    process::{
        EngineInstance, HostError,
        windows::{
            files::FileConfig32,
            gdi::{GdiConfig32, GdiRuntime32},
        },
    },
    windows::{
        WindowsApi32, files::FileApi32 as File, gdi::GdiApi32 as Gdi, user32::User32Api32 as Api,
    },
};
fn engine() -> EngineInstance {
    let (mut engine, _) = fixture();
    engine.gdi = Some(
        GdiRuntime32::new(GdiConfig32 {
            objects: 16,
            dcs: 4,
            saved_states: 4,
            bitmap_base: 0x200000,
            bitmap_pages: 8,
            bitmap_bytes: 65536,
            screen_width: 8,
            screen_height: 8,
        })
        .unwrap(),
    );
    assert_eq!(call(&mut engine, Api::OpenClipboard.id(), &[0]), 1);
    assert_eq!(call(&mut engine, Api::EmptyClipboard.id(), &[]), 1);
    engine
}
fn words(engine: &mut EngineInstance, at: u32, input: &[u32]) {
    let bytes = input
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .collect::<Vec<_>>();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(at), &bytes)
        .unwrap();
}
fn dib(engine: &mut EngineInstance, section: u32, offset: u32) -> (u32, u32) {
    words(
        engine,
        0x11000,
        &[40, 2, (-2i32) as u32, 1 | 32 << 16, 0, 16, 0, 0, 0, 0],
    );
    let bitmap = call(
        engine,
        Gdi::CreateDIBSection.id(),
        &[0, 0x11000, 0, 0x11080, section, offset],
    );
    assert_ne!(bitmap, 0);
    let mut bits = [0; 4];
    engine
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(0x11080), &mut bits)
        .unwrap();
    (bitmap, u32::from_le_bytes(bits))
}
#[test]
fn captured_bitmap_and_palette_transfer_allow_selection_and_reads_but_reject_guest_mutation_and_delete()
 {
    let mut engine = engine();
    words(&mut engine, 0x11000, &[0x00ff0000, 0x000000ff]);
    let bitmap = call(&mut engine, Gdi::CreateBitmap.id(), &[2, 1, 1, 32, 0x11000]);
    assert_ne!(bitmap, 0);
    words(&mut engine, 0x11000, &[0x00020300, 0x000000ff, 0x0000ff00]);
    let palette = call(&mut engine, Gdi::CreatePalette.id(), &[0x11000]);
    assert_ne!(palette, 0);
    assert_eq!(
        call(&mut engine, Api::SetClipboardData.id(), &[2, bitmap]),
        bitmap
    );
    assert_eq!(
        call(&mut engine, Api::SetClipboardData.id(), &[9, palette]),
        palette
    );
    assert_eq!(call(&mut engine, Api::GetClipboardData.id(), &[2]), bitmap);
    assert_eq!(call(&mut engine, Api::GetClipboardData.id(), &[9]), palette);
    let dc = call(&mut engine, Gdi::CreateCompatibleDC.id(), &[0]);
    assert_ne!(call(&mut engine, Gdi::SelectObject.id(), &[dc, bitmap]), 0);
    assert_ne!(
        call(&mut engine, Gdi::SelectPalette.id(), &[dc, palette, 0]),
        0
    );
    assert_eq!(call(&mut engine, Gdi::GetPixel.id(), &[dc, 0, 0]), 255);
    assert_eq!(
        call(
            &mut engine,
            Gdi::GetPaletteEntries.id(),
            &[palette, 0, 2, 0x11100]
        ),
        2
    );
    assert_eq!(call(&mut engine, Gdi::DeleteObject.id(), &[bitmap]), 0);
    assert_eq!(call(&mut engine, Gdi::DeleteObject.id(), &[palette]), 0);
    let mut before = [0; 8];
    assert_eq!(
        call(&mut engine, Gdi::GetBitmapBits.id(), &[bitmap, 8, 0x11100]),
        8
    );
    engine
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(0x11100), &mut before)
        .unwrap();
    assert_eq!(
        call(&mut engine, Gdi::SetBitmapBits.id(), &[bitmap, 8, 0x11000]),
        0
    );
    assert_eq!(
        call(&mut engine, Gdi::SetPixel.id(), &[dc, 0, 0, 0]),
        u32::MAX
    );
    assert_eq!(
        call(
            &mut engine,
            Gdi::SetPaletteEntries.id(),
            &[palette, 0, 1, 0x11004]
        ),
        0
    );
    assert_eq!(
        call(&mut engine, Gdi::GetBitmapBits.id(), &[bitmap, 8, 0x11100]),
        8
    );
    let mut after = [0; 8];
    engine
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(0x11100), &mut after)
        .unwrap();
    assert_eq!(after, before);
    call(&mut engine, Gdi::SaveDC.id(), &[dc]);
    assert_eq!(call(&mut engine, Api::EmptyClipboard.id(), &[]), 1);
    assert_eq!(
        call(&mut engine, Gdi::GetPixel.id(), &[dc, 0, 0]),
        255,
        "live selected DC retains read authority after clipboard releases its lease"
    );
    assert_eq!(call(&mut engine, Gdi::DeleteDC.id(), &[dc]), 1);
    assert_eq!(call(&mut engine, Gdi::GetObjectW.id(), &[bitmap, 0, 0]), 0);
    assert_eq!(
        call(
            &mut engine,
            Gdi::GetPaletteEntries.id(),
            &[palette, 0, 0, 0]
        ),
        0
    );
    for _ in 0..24 {
        let bitmap = call(&mut engine, Gdi::CreateBitmap.id(), &[2, 1, 1, 32, 0]);
        assert_ne!(bitmap, 0);
        assert_eq!(
            call(&mut engine, Api::SetClipboardData.id(), &[2, bitmap]),
            bitmap
        );
        assert_eq!(call(&mut engine, Api::EmptyClipboard.id(), &[]), 1);
        assert_eq!(call(&mut engine, Gdi::GetObjectW.id(), &[bitmap, 0, 0]), 0);
    }
}
#[test]
fn clipboard_bitmap_replace_unmaps_private_dib_only_after_checked_atomic_retirement_and_retry() {
    let mut engine = engine();
    let (old, bits) = dib(&mut engine, 0, 0);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(bits), &[1, 2, 3, 4])
        .unwrap();
    assert_eq!(
        call(&mut engine, Api::SetClipboardData.id(), &[2, old]),
        old
    );
    let next = call(&mut engine, Gdi::CreateBitmap.id(), &[2, 1, 1, 32, 0]);
    assert_ne!(next, 0);
    let sequence = call(&mut engine, Api::GetClipboardSequenceNumber.id(), &[]);
    let token = super::capture_only(&mut engine, Api::SetClipboardData.id(), &[2, next]);
    let version = engine.memory.as_ref().unwrap().transaction_version();
    engine.memory.as_mut().unwrap().exhaust_versions_for_test();
    assert_eq!(
        engine.complete_windows_call(KEY, engine.generation, token),
        Err(HostError::Memory(MemoryError::VersionExhausted))
    );
    assert_eq!(
        engine.user32.as_ref().unwrap().clipboard.sequence(),
        sequence
    );
    assert_eq!(
        engine
            .user32
            .as_ref()
            .unwrap()
            .clipboard
            .get(2)
            .unwrap()
            .gdi()
            .unwrap()
            .handle(),
        old
    );
    let mut bytes = [0; 4];
    engine
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(bits), &mut bytes)
        .unwrap();
    assert_eq!(bytes, [1, 2, 3, 4]);
    assert!(
        engine
            .gdi
            .as_ref()
            .unwrap()
            .bitmap_info(engine.memory.as_ref().unwrap(), next)
            .is_ok()
    );
    engine
        .memory
        .as_mut()
        .unwrap()
        .restore_version_for_test(version);
    engine
        .complete_windows_call(KEY, engine.generation, token)
        .unwrap();
    assert!(engine.pending_call.is_none());
    assert!(matches!(
        engine
            .memory
            .as_ref()
            .unwrap()
            .read(GuestAddress(bits), &mut bytes),
        Err(MemoryError::Fault(fault)) if fault.reason == crate::memory::FaultReason::Unmapped
    ));
    assert_eq!(call(&mut engine, Api::GetClipboardData.id(), &[2]), next);
}
#[test]
fn clipboard_section_bitmap_retirement_preserves_an_independent_alias_and_releases_its_own_view() {
    let mut engine = engine();
    engine.configure_files(FileConfig32::default()).unwrap();
    let section = call(
        &mut engine,
        File::CreateFileMappingW.id(),
        &[u32::MAX, 0, 4, 0, 4096, 0],
    );
    assert_ne!(section, 0);
    let keeper = call(
        &mut engine,
        File::MapViewOfFile.id(),
        &[section, 2, 0, 0, 4096],
    );
    assert_ne!(keeper, 0);
    let (bitmap, bits) = dib(&mut engine, section, 4);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(bits), &[0x12, 0x34, 0x56, 0x78])
        .unwrap();
    assert_eq!(
        call(&mut engine, WindowsApi32::CloseHandle.id(), &[section]),
        1
    );
    assert_eq!(
        call(&mut engine, Api::SetClipboardData.id(), &[2, bitmap]),
        bitmap
    );
    assert_eq!(call(&mut engine, Api::EmptyClipboard.id(), &[]), 1);
    let mut bytes = [0; 4];
    assert!(
        engine
            .memory
            .as_ref()
            .unwrap()
            .read(GuestAddress(bits), &mut bytes)
            .is_err()
    );
    engine
        .memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(keeper + 4), &mut bytes)
        .unwrap();
    assert_eq!(bytes, [0x12, 0x34, 0x56, 0x78]);
    assert_eq!(call(&mut engine, Gdi::GetObjectW.id(), &[bitmap, 0, 0]), 0);
    assert_eq!(call(&mut engine, File::UnmapViewOfFile.id(), &[keeper]), 1);
    for _ in 0..24 {
        let section = call(
            &mut engine,
            File::CreateFileMappingW.id(),
            &[u32::MAX, 0, 4, 0, 4096, 0],
        );
        assert_ne!(section, 0);
        let (bitmap, bits) = dib(&mut engine, section, 4);
        assert_eq!(
            call(&mut engine, WindowsApi32::CloseHandle.id(), &[section]),
            1
        );
        assert_eq!(
            call(&mut engine, Api::SetClipboardData.id(), &[2, bitmap]),
            bitmap
        );
        assert_eq!(call(&mut engine, Api::EmptyClipboard.id(), &[]), 1);
        assert!(
            engine
                .memory
                .as_ref()
                .unwrap()
                .read(GuestAddress(bits), &mut bytes)
                .is_err()
        );
    }
}
