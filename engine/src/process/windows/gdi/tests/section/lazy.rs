use super::*;
use crate::{
    files::{AssetRef32, FileTimes32},
    process::{HostError, calls::call::PendingOwner},
};
fn lazy_file(engine: &mut EngineInstance) -> u32 {
    let path: Vec<_> = "C:\\pixels.bin".encode_utf16().collect();
    engine
        .mount_file(
            &path,
            AssetRef32 {
                id: 302,
                length: 8192,
            },
            0x80,
            FileTimes32::default(),
        )
        .unwrap();
    let path_bytes = path
        .into_iter()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x11400), &path_bytes)
        .unwrap();
    let handle = call(
        engine,
        FileApi32::CreateFileW.id(),
        &[0x11400, 0xc0000000, 3, 0, 3, 0x80, 0],
    );
    assert_ne!(handle, u32::MAX);
    handle
}
#[test]
fn lazy_section_dib_retries_exact_file_owner_then_flushes_shared_pixels_and_cancels_without_object()
{
    let mut engine = fixture();
    let file = lazy_file(&mut engine);
    let section = call(
        &mut engine,
        FileApi32::CreateFileMappingW.id(),
        &[file, 0, 4, 0, 0, 0],
    );
    assert_ne!(section, 0);
    let token = capture(
        &mut engine,
        GdiApi32::CreateDIBSection.id(),
        &[0, 0x11080, 0, 0x11100, section, 4],
    );
    let mapped = engine.memory().unwrap().mapped_pages();
    let version = engine.memory().unwrap().transaction_version();
    finish(&mut engine, token);
    assert_eq!(
        engine.pending_call.as_ref().unwrap().owner,
        PendingOwner::Replacement(engine.generation)
    );
    assert_eq!(engine.pending_file_call().unwrap().token, token);
    assert_eq!(engine.memory().unwrap().mapped_pages(), mapped);
    assert_eq!(engine.memory().unwrap().transaction_version(), version);
    assert_eq!(engine.gdi.as_ref().unwrap().model.object_count(), 0);
    let cancelled = engine.pending_file_request().unwrap().unwrap();
    engine.cancel_file_call(7, token).unwrap();
    assert!(engine.pending_file_call().is_none());
    assert!(engine.pending_call.is_none());
    assert!(matches!(
        engine.supply_file_bytes(cancelled, 0, &[0]),
        Err(HostError::File(crate::files::FileError32::Stale))
    ));
    let token = capture(
        &mut engine,
        GdiApi32::CreateDIBSection.id(),
        &[0, 0x11080, 0, 0x11100, section, 4],
    );
    finish(&mut engine, token);
    let request = engine.pending_file_request().unwrap().unwrap();
    assert_ne!(request.request, cancelled.request);
    let mut source = vec![0x19; request.length as usize];
    source[4..8].copy_from_slice(&[0x33, 0x22, 0x11, 0]);
    for offset in (0..request.length).step_by(4096) {
        let count = (request.length - offset).min(4096) as usize;
        engine
            .supply_file_bytes(
                request,
                offset,
                &source[offset as usize..offset as usize + count],
            )
            .unwrap();
    }
    engine.resume_file_call(7, token).unwrap();
    assert!(engine.pending_file_call().is_none());
    assert!(engine.pending_call.is_none());
    let bitmap = decode_state(&engine.arena()[..STATE_SIZE])
        .unwrap()
        .registers[0];
    assert_ne!(bitmap, 0);
    let bits = word(&engine, 0x11100);
    assert_eq!(read::<4>(&engine, bits), [0x33, 0x22, 0x11, 0]);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(bits), &[0x66, 0x55, 0x44, 0])
        .unwrap();
    assert_eq!(
        call(&mut engine, WindowsApi32::CloseHandle.id(), &[section]),
        1
    );
    assert_eq!(call(&mut engine, GdiApi32::DeleteObject.id(), &[bitmap]), 1);
    assert_eq!(
        call(
            &mut engine,
            FileApi32::SetFilePointer.id(),
            &[file, 4, 0, 0]
        ),
        4
    );
    assert_eq!(
        call(
            &mut engine,
            FileApi32::ReadFile.id(),
            &[file, 0x11300, 4, 0x11320, 0]
        ),
        1
    );
    assert_eq!(word(&engine, 0x11320), 4);
    assert_eq!(read::<4>(&engine, 0x11300), [0x66, 0x55, 0x44, 0]);
}
