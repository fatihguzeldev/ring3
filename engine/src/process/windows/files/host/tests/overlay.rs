use super::*;
use crate::files::FileIo32;
#[test]
fn long_overlay_path_pages_are_revision_bound_and_failure_neutral() {
    let mut engine = engine();
    let epoch = engine.files_epoch().unwrap();
    let path = long_path();
    let pending = engine
        .begin_file_mount(
            epoch,
            FileMountStart32 {
                units: path.len() as u32,
                asset: Some(AssetRef32 { id: 502, length: 0 }),
                attributes: 0x80,
                times: FileTimes32::default(),
            },
        )
        .unwrap();
    append(&mut engine, pending, &path);
    let file = engine.finish_file_mount(epoch, pending.token).unwrap();
    let files = engine.files.as_mut().unwrap();
    let FileIo32::Ready(write) = files.vfs.prepare_write_at(file, 0, b"x").unwrap() else {
        panic!()
    };
    files.vfs.commit_write(write);
    let entry = engine.file_overlay_entry(epoch, 0).unwrap().unwrap();
    assert!(entry.path.len() > 2000);
    let mut reconstructed = Vec::new();
    let mut offset = 0;
    loop {
        let mut chunk = [0u16; 2032];
        let count = engine
            .read_file_overlay_path(epoch, entry.revision, 0, offset, &mut chunk)
            .unwrap();
        reconstructed.extend_from_slice(&chunk[..count]);
        offset += count as u32;
        if count == 0 {
            break;
        }
    }
    assert_eq!(reconstructed, entry.path);
    let files = engine.files.as_mut().unwrap();
    let attrs = files.vfs.prepare_set_attributes(&path, 2).unwrap();
    files.vfs.commit_namespace(attrs);
    let mut output = [0xabcd; 3];
    assert_eq!(
        engine.read_file_overlay_path(epoch, entry.revision, 0, 0, &mut output),
        Err(HostError::File(FileError32::Stale))
    );
    assert_eq!(output, [0xabcd; 3]);
}
#[test]
fn cancelling_host_export_request_reclaims_stage_and_rejects_late_supply() {
    let mut engine = engine();
    let epoch = engine.files_epoch().unwrap();
    let path = utf16("C:\\export");
    let file = engine
        .mount_file(
            &path,
            AssetRef32 {
                id: 503,
                length: 10000,
            },
            0x80,
            FileTimes32::default(),
        )
        .unwrap();
    let files = engine.files.as_mut().unwrap();
    let attrs = files.vfs.prepare_set_attributes(&path, 2).unwrap();
    files.vfs.commit_namespace(attrs);
    let revision = files.vfs.revision();
    let mut output = [0xa5; 32];
    assert_eq!(
        engine
            .read_file_overlay(epoch, file, revision, 0, &mut output)
            .unwrap(),
        None
    );
    assert_eq!(output, [0xa5; 32]);
    let request = engine.pending_file_request().unwrap().unwrap();
    let stale = FileReadRequest32 {
        epoch: epoch + 1,
        ..request
    };
    assert_eq!(
        engine.cancel_file_request(stale),
        Err(HostError::File(FileError32::Stale))
    );
    assert_eq!(engine.pending_file_request().unwrap(), Some(request));
    engine.cancel_file_request(request).unwrap();
    assert!(engine.pending_file_request().unwrap().is_none());
    assert_eq!(
        engine.supply_file_bytes(request, 0, &[1]),
        Err(HostError::File(FileError32::Stale))
    );
    assert_eq!(
        engine
            .read_file_overlay(epoch, file, revision, 0, &mut output)
            .unwrap(),
        None
    );
    let next = engine.pending_file_request().unwrap().unwrap();
    assert!(next.request > request.request);
    for offset in (0..next.length).step_by(4096) {
        let count = (next.length - offset).min(4096) as usize;
        engine
            .supply_file_bytes(next, offset, &vec![7; count])
            .unwrap();
    }
    assert_eq!(
        engine
            .read_file_overlay(epoch, file, revision, 0, &mut output)
            .unwrap(),
        Some(32)
    );
    assert_eq!(output, [7; 32]);
}
#[test]
fn cancelling_export_fetch_does_not_cancel_identical_guest_read_stage() {
    let mut engine = engine();
    let path = utf16("C:\\independent");
    let file = engine
        .mount_file(
            &path,
            AssetRef32 {
                id: 504,
                length: 10000,
            },
            0x80,
            FileTimes32::default(),
        )
        .unwrap();
    let files = engine.files.as_mut().unwrap();
    let attrs = files.vfs.prepare_set_attributes(&path, 2).unwrap();
    files.vfs.commit_namespace(attrs);
    let revision = files.vfs.revision();
    let FileIo32::NeedBytes(guest) = files.vfs.prepare_read_at(file, 0, 32).unwrap() else {
        panic!()
    };
    let FileIo32::NeedBytes(export) = files.vfs.read_overlay(file, revision, 0, 32).unwrap() else {
        panic!()
    };
    assert_ne!(guest.request, export.request);
    engine.cancel_file_request(export).unwrap();
    for offset in (0..guest.length).step_by(4096) {
        let count = (guest.length - offset).min(4096) as usize;
        engine
            .supply_file_bytes(guest, offset, &vec![9; count])
            .unwrap();
    }
    let files = engine.files.as_mut().unwrap();
    let FileIo32::Ready(read) = files.vfs.prepare_read_at(file, 0, 32).unwrap() else {
        panic!()
    };
    assert_eq!(read.bytes(), &[9; 32]);
    assert_eq!(
        engine.supply_file_bytes(export, 0, &[9]),
        Err(HostError::File(FileError32::Stale))
    );
}
