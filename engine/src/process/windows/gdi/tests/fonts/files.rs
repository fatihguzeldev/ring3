use super::super::paint::{KEY, capture, eax, fixture_with_startup};
use crate::{
    files::{ATTR_NORMAL, AssetRef32, FileTimes32},
    memory::GuestAddress,
    process::{CallError, EngineInstance, HostError, windows::files::FileConfig32},
    windows::gdi::GdiApi32 as Api,
};
const PATH: u32 = 0x12a00;
fn fixture() -> (EngineInstance, Vec<u8>) {
    let (mut engine, _, _) = fixture_with_startup(false);
    engine
        .configure_files(FileConfig32 {
            vfs: crate::files::FileLimits32 {
                max_io_bytes: 4096,
                ..Default::default()
            },
            ..Default::default()
        })
        .unwrap();
    let bytes = crate::windows::gdi::private_font_source_for_test();
    engine
        .mount_file(
            &r"C:\face.ttf".encode_utf16().collect::<Vec<_>>(),
            AssetRef32 {
                id: 909,
                length: bytes.len() as u64,
            },
            ATTR_NORMAL,
            FileTimes32::default(),
        )
        .unwrap();
    path(&mut engine, r"C:\face.ttf");
    (engine, bytes)
}
fn path(engine: &mut EngineInstance, value: &str) {
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(
            GuestAddress(PATH),
            &value
                .encode_utf16()
                .chain([0])
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
}
fn feed(engine: &mut EngineInstance, bytes: &[u8], token: u32) -> usize {
    let mut waits = 0;
    while let Some(wait) = engine.pending_file_call() {
        assert_eq!(wait.token, token);
        let request = engine
            .pending_file_request()
            .unwrap()
            .expect("real lazy font request");
        assert_eq!(request.asset, 909);
        let start = request.offset as usize;
        for (index, part) in bytes[start..start + request.length as usize]
            .chunks(4096)
            .enumerate()
        {
            engine
                .supply_file_bytes(request, index as u32 * 4096, part)
                .unwrap();
        }
        engine.resume_file_call(KEY, token).unwrap();
        waits += 1;
    }
    waits
}

fn file_probe(engine: &mut EngineInstance, desired: u32) -> u32 {
    capture(
        engine,
        crate::windows::files::FileApi32::CreateFileW.id(),
        &[PATH, desired, 7, 0, 3, 0, 0],
    );
    eax(engine)
}
fn close_probe(engine: &mut EngineInstance, handle: u32) {
    capture(
        engine,
        crate::windows::WindowsApi32::CloseHandle.id(),
        &[handle],
    );
    assert_eq!(eax(engine), 1);
}

#[test]
fn captured_file_font_blocks_write_but_not_read_delete_until_last_remove() {
    let (mut engine, bytes) = fixture();
    engine
        .configure_synchronization(Default::default())
        .unwrap();
    let writer = file_probe(&mut engine, 0x40000000);
    assert_ne!(writer, u32::MAX);
    let (_, token) = capture(&mut engine, Api::AddFontResourceExW.id(), &[PATH, 0x10, 0]);
    assert_eq!(eax(&engine), 0);
    assert!(engine.pending_call.is_none());
    assert!(engine.gdi.as_ref().unwrap().font_file_calls.is_empty());
    assert!(engine.gdi.as_ref().unwrap().font_file_sources.is_empty());
    assert_ne!(token, 0);
    close_probe(&mut engine, writer);
    for _ in 0..2 {
        let (_, token) = capture(&mut engine, Api::AddFontResourceExW.id(), &[PATH, 0x10, 0]);
        feed(&mut engine, &bytes, token);
        assert_eq!(eax(&engine), 1);
    }
    for access in [0x80000000, 0x00010000] {
        let handle = file_probe(&mut engine, access);
        assert_ne!(handle, u32::MAX);
        close_probe(&mut engine, handle);
    }
    assert_eq!(file_probe(&mut engine, 0x40000000), u32::MAX);
    assert_eq!(engine.current_windows_thread().unwrap().last_error(), 32);
    capture(
        &mut engine,
        Api::RemoveFontResourceExW.id(),
        &[PATH, 0x10, 0],
    );
    assert_eq!(eax(&engine), 1);
    assert_eq!(file_probe(&mut engine, 0x40000000), u32::MAX);
    capture(
        &mut engine,
        Api::RemoveFontResourceExW.id(),
        &[PATH, 0x10, 0],
    );
    assert_eq!(eax(&engine), 1);
    let writer = file_probe(&mut engine, 0x40000000);
    assert_ne!(writer, u32::MAX);
    close_probe(&mut engine, writer);
}

#[test]
fn removed_file_source_hold_tracks_saved_and_metric_face_owners_until_real_release() {
    let (mut engine, bytes) = fixture();
    engine
        .configure_synchronization(Default::default())
        .unwrap();
    let (_, token) = capture(&mut engine, Api::AddFontResourceExW.id(), &[PATH, 0x10, 0]);
    feed(&mut engine, &bytes, token);
    assert_eq!(eax(&engine), 1);
    let gdi = engine.gdi.as_mut().unwrap();
    let source = gdi.font_file_sources[0].source;
    let font = gdi
        .create_logical_font(crate::windows::gdi::LogicalFont32 {
            face: "Test Mono Face".encode_utf16().collect(),
            ..Default::default()
        })
        .unwrap();
    let ram = engine.memory.as_mut().unwrap();
    let dc = super::super::call(gdi, ram, Api::CreateCompatibleDC, &[0]).unwrap();
    let previous = super::super::call(gdi, ram, Api::SelectObject, &[dc, font]).unwrap();
    super::super::call(gdi, ram, Api::SaveDC, &[dc]).unwrap();
    super::super::call(gdi, ram, Api::SelectObject, &[dc, previous]).unwrap();
    capture(
        &mut engine,
        Api::RemoveFontResourceExW.id(),
        &[PATH, 0x10, 0],
    );
    assert_eq!(eax(&engine), 1);
    engine.collect_gdi_font_file_holds().unwrap();
    let gdi = engine.gdi.as_ref().unwrap();
    assert!(gdi.fonts.source_is_live(source));
    assert_eq!(gdi.font_file_sources.len(), 1);
    assert_eq!(gdi.font_file_sources[0].references, 0);
    assert!(matches!(
        engine.open_provider_file(&r"C:\face.ttf".encode_utf16().collect::<Vec<_>>(), true),
        Err(HostError::File(crate::files::FileError32::SharingViolation))
    ));
    let gdi = engine.gdi.as_mut().unwrap();
    let ram = engine.memory.as_mut().unwrap();
    super::super::call(gdi, ram, Api::RestoreDC, &[dc, u32::MAX]).unwrap();
    super::super::call(gdi, ram, Api::SelectObject, &[dc, previous]).unwrap();
    super::super::call(gdi, ram, Api::DeleteObject, &[font]).unwrap();
    assert!(!gdi.fonts.source_is_live(source));
    let writer = file_probe(&mut engine, 0x40000000);
    assert_ne!(writer, u32::MAX);
    assert!(engine.gdi.as_ref().unwrap().font_file_sources.is_empty());
    close_probe(&mut engine, writer);
}
#[test]
fn captured_file_font_uses_original_lazy_path_exact_file_wait_and_repeated_reference_contract() {
    let (mut engine, bytes) = fixture();
    let (_, token) = capture(&mut engine, Api::AddFontResourceExW.id(), &[PATH, 0x10, 0]);
    assert_eq!(engine.pending_file_call().unwrap().token, token);
    assert_eq!(
        engine.resume_file_call(KEY + 1, token),
        Err(HostError::InvalidArtifact)
    );
    assert_eq!(
        engine.resume_file_call(KEY, token + 1),
        Err(HostError::Call(CallError::InvalidToken))
    );
    path(&mut engine, r"C:\wrong.ttf");
    assert!(feed(&mut engine, &bytes, token) > 0);
    assert_eq!(eax(&engine), 1);
    assert!(engine.pending_call.is_none());
    assert_eq!(engine.gdi.as_ref().unwrap().font_file_sources.len(), 1);
    path(&mut engine, r"c:\FACE.ttf");
    let (_, token) = capture(&mut engine, Api::AddFontResourceExW.id(), &[PATH, 0x10, 0]);
    feed(&mut engine, &bytes, token);
    assert_eq!(eax(&engine), 1);
    let gdi = engine.gdi.as_ref().unwrap();
    assert_eq!(gdi.font_file_sources.len(), 1);
    assert_eq!(gdi.font_file_sources[0].references, 2);
    capture(&mut engine, Api::RemoveFontResourceExW.id(), &[PATH, 0, 0]);
    assert_eq!(eax(&engine), 0);
    assert_eq!(
        engine.gdi.as_ref().unwrap().font_file_sources[0].references,
        2
    );
    for remaining in [1, 0] {
        capture(
            &mut engine,
            Api::RemoveFontResourceExW.id(),
            &[PATH, 0x10, 0],
        );
        assert_eq!(eax(&engine), 1);
        assert_eq!(
            engine.gdi.as_ref().unwrap().font_file_sources.len(),
            remaining
        );
    }
    capture(
        &mut engine,
        Api::RemoveFontResourceExW.id(),
        &[PATH, 0x10, 0],
    );
    assert_eq!(eax(&engine), 0);
}
#[test]
fn cancelled_file_font_cannot_publish_face_and_releases_only_its_canonical_provider_request() {
    let (mut engine, _) = fixture();
    engine
        .mount_file(
            &r"C:\other.ttf".encode_utf16().collect::<Vec<_>>(),
            AssetRef32 {
                id: 910,
                length: 17,
            },
            ATTR_NORMAL,
            FileTimes32::default(),
        )
        .unwrap();
    let other = engine
        .open_provider_file(&r"C:\other.ttf".encode_utf16().collect::<Vec<_>>(), false)
        .unwrap();
    let (_, token) = capture(&mut engine, Api::AddFontResourceW.id(), &[PATH]);
    let request = engine.pending_file_request().unwrap().unwrap();
    assert_eq!(engine.gdi.as_ref().unwrap().font_file_calls.len(), 1);
    engine.cancel_file_call(KEY, token).unwrap();
    assert!(engine.pending_call.is_none());
    assert_eq!(engine.provider_file_size(other), Ok(17));
    assert!(engine.pending_file_call().is_none());
    assert!(engine.pending_file_request().unwrap().is_none());
    assert!(engine.gdi.as_ref().unwrap().font_file_calls.is_empty());
    assert!(engine.gdi.as_ref().unwrap().font_file_sources.is_empty());
    assert!(engine.supply_file_bytes(request, 0, &[0; 12]).is_err());
    assert!(
        engine
            .open_provider_file(&r"C:\face.ttf".encode_utf16().collect::<Vec<_>>(), true)
            .is_ok()
    );
}
#[test]
fn file_font_handle_cannot_be_removed_through_memory_resource_api_and_not_enum_is_honest() {
    let (mut engine, bytes) = fixture();
    let before = engine
        .gdi
        .as_ref()
        .unwrap()
        .fonts
        .enumerable_fonts()
        .unwrap()
        .len();
    let (_, token) = capture(&mut engine, Api::AddFontResourceExW.id(), &[PATH, 0x30, 0]);
    feed(&mut engine, &bytes, token);
    assert_eq!(eax(&engine), 1);
    let source = engine.gdi.as_ref().unwrap().font_file_sources[0].source;
    assert_eq!(
        engine
            .gdi
            .as_ref()
            .unwrap()
            .fonts
            .enumerable_fonts()
            .unwrap()
            .len(),
        before
    );
    capture(&mut engine, Api::RemoveFontMemResourceEx.id(), &[source]);
    assert_eq!(eax(&engine), 0);
    assert_eq!(engine.gdi.as_ref().unwrap().font_file_sources.len(), 1);
    capture(
        &mut engine,
        Api::RemoveFontResourceExW.id(),
        &[PATH, 0x30, 0],
    );
    assert_eq!(eax(&engine), 1);
}
