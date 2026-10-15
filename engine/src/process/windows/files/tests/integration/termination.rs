use super::*;
use crate::{
    files::{AssetRef32, FileError32, FileIo32, FileLimits32, FileTimes32, Vfs32},
    process::windows::files::tests::support,
    windows::{WindowsApi32 as Kernel, files::FileApi32 as File},
};

fn mapping(engine: &mut EngineInstance, file: u32, length: u32) -> (u32, u32) {
    let section = complete(
        engine,
        File::CreateFileMappingW.id(),
        &[file, 0, 4, 0, length, 0],
    );
    let view = complete(
        engine,
        File::MapViewOfFile.id(),
        &[section, 2, 0, 0, length],
    );
    (section, view)
}

#[test]
fn exit_flushes_shared_views_discards_private_pages_and_cancels_open_async_ownership() {
    let mut engine = engine();
    let file = file(&mut engine, false);
    let open = engine.files.as_ref().unwrap().handles.file(file).unwrap();
    let id = engine
        .files
        .as_ref()
        .unwrap()
        .vfs
        .file_for_open(open)
        .unwrap();
    let (section, shared) = mapping(&mut engine, file, 4096);
    let private = complete(
        &mut engine,
        File::MapViewOfFile.id(),
        &[section, 1, 0, 0, 4096],
    );
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(shared + 3), &[9])
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(private + 4), &[7])
        .unwrap();
    support::path(
        engine.memory.as_mut().unwrap(),
        0x1400,
        r"C:\delete-at-exit.tmp",
    );
    let temporary = complete(
        &mut engine,
        File::CreateFileW.id(),
        &[0x1400, 0xc001_0000, 7, 0, 4, 0x0400_0080, 0],
    );
    assert_ne!(temporary, u32::MAX);
    support::path(engine.memory.as_mut().unwrap(), 0x1800, r"C:\pending.bin");
    let asynchronous = complete(
        &mut engine,
        File::CreateFileW.id(),
        &[0x1800, 0xc000_0000, 7, 0, 4, 0x4000_0080, 0],
    );
    let async_open = engine
        .files
        .as_ref()
        .unwrap()
        .handles
        .file(asynchronous)
        .unwrap();
    let async_id = engine
        .files
        .as_ref()
        .unwrap()
        .vfs
        .file_for_open(async_open)
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1300), &[5])
        .unwrap();
    assert_eq!(
        complete(
            &mut engine,
            File::WriteFile.id(),
            &[asynchronous, 0x1300, 1, 0, 0x1100]
        ),
        0
    );
    assert_eq!(engine.windows_thread.last_error(), 997);
    assert_eq!(complete(&mut engine, Kernel::ExitProcess.id(), &[23]), 0);
    assert_eq!(engine.exit_code, Some(23));
    assert!(engine.pending_call.is_none());
    let files = engine.files.as_mut().unwrap();
    assert!(files.events.is_empty());
    assert!(files.handles.object(file).is_err());
    assert!(files.handles.object(section).is_err());
    assert!(files.vfs.open_info(open).is_err());
    assert!(files.vfs.open_info(async_open).is_err());
    assert_eq!(files.vfs.info(async_id).unwrap().size, 0);
    assert_eq!(
        files
            .vfs
            .path_info(&r"C:\delete-at-exit.tmp".encode_utf16().collect::<Vec<_>>()),
        Err(FileError32::NotFound)
    );
    let FileIo32::Ready(bytes) = files.vfs.prepare_read_at(id, 0, 8).unwrap() else {
        panic!("written overlay is resident")
    };
    assert_eq!(bytes.bytes(), &[0, 0, 0, 9, 0, 0, 0, 0]);
    assert!(
        engine
            .memory
            .as_ref()
            .unwrap()
            .permissions_at(GuestAddress(shared))
            .is_err()
    );
    assert!(
        engine
            .memory
            .as_ref()
            .unwrap()
            .permissions_at(GuestAddress(private))
            .is_err()
    );
}

fn supply(engine: &mut EngineInstance, request: crate::files::FileReadRequest32) {
    let bytes = [0x11; 4096];
    for offset in (0..request.length).step_by(4096) {
        let length = (request.length - offset).min(4096) as usize;
        engine
            .supply_file_bytes(request, offset, &bytes[..length])
            .unwrap();
    }
}

#[test]
fn exit_parks_dirty_asset_writeback_and_resumes_the_original_token_before_teardown() {
    let mut engine = engine();
    let epoch = engine.files_epoch().unwrap();
    engine.files.as_mut().unwrap().vfs = Vfs32::new(
        epoch,
        FileLimits32 {
            cache_pages: 1,
            ..FileLimits32::default()
        },
    )
    .unwrap();
    let path: Vec<u16> = r"C:\asset.bin".encode_utf16().collect();
    engine
        .mount_file(
            &path,
            AssetRef32 {
                id: 29,
                length: 131_072,
            },
            0x80,
            FileTimes32::default(),
        )
        .unwrap();
    support::path(engine.memory.as_mut().unwrap(), 0x1000, r"C:\asset.bin");
    let file = complete(
        &mut engine,
        File::CreateFileW.id(),
        &[0x1000, 0xc000_0000, 7, 0, 3, 0x80, 0],
    );
    let section = complete(
        &mut engine,
        File::CreateFileMappingW.id(),
        &[file, 0, 4, 0, 0, 0],
    );
    let token = capture(
        &mut engine,
        File::MapViewOfFile.id(),
        &[section, 2, 0, 0, 4096],
        None,
    );
    engine
        .complete_windows_call(KEY, engine.generation, token)
        .unwrap();
    let request = engine.pending_file_request().unwrap().unwrap();
    supply(&mut engine, request);
    engine.resume_file_call(KEY, token).unwrap();
    let view = decode_state(&engine.arena()[..STATE_SIZE])
        .unwrap()
        .registers[0];
    let files = engine.files.as_mut().unwrap();
    let id = files
        .vfs
        .file_for_open(files.handles.file(file).unwrap())
        .unwrap();
    let FileIo32::NeedBytes(request) = files.vfs.prepare_read_at(id, 65_536, 1).unwrap() else {
        panic!("second source page must evict the first")
    };
    supply(&mut engine, request);
    let files = engine.files.as_mut().unwrap();
    let FileIo32::Ready(read) = files.vfs.prepare_read_at(id, 65_536, 1).unwrap() else {
        panic!("second page supplied")
    };
    files.vfs.commit_read(read);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(view + 3), &[9])
        .unwrap();
    let token = capture(&mut engine, Kernel::ExitProcess.id(), &[31], None);
    engine
        .complete_windows_call(KEY, engine.generation, token)
        .unwrap();
    assert_eq!(engine.exit_code, None);
    assert_eq!(engine.pending_file_call().unwrap().token, token);
    assert!(engine.files.as_ref().unwrap().handles.object(file).is_ok());
    let request = engine.pending_file_request().unwrap().unwrap();
    let before = engine.arena().to_vec();
    assert_eq!(
        engine.cancel_file_call(KEY, token),
        Err(crate::process::HostError::Call(
            crate::process::CallError::Busy
        ))
    );
    assert_eq!(engine.arena(), before);
    assert_eq!(engine.pending_file_request().unwrap(), Some(request));
    assert_eq!(engine.pending_file_call().unwrap().token, token);
    supply(&mut engine, request);
    engine.resume_file_call(KEY, token).unwrap();
    assert_eq!(engine.exit_code, Some(31));
    assert!(engine.pending_call.is_none());
    assert!(engine.files.as_ref().unwrap().reply.is_none());
    let FileIo32::Ready(read) = engine
        .files
        .as_mut()
        .unwrap()
        .vfs
        .prepare_read_at(id, 0, 8)
        .unwrap()
    else {
        panic!("writeback owns the source page")
    };
    assert_eq!(read.bytes(), &[0x11, 0x11, 0x11, 9, 0x11, 0x11, 0x11, 0x11]);
}

#[test]
fn exit_writeback_chunks_dirty_views_to_the_configured_io_limit() {
    let mut engine = engine();
    let epoch = engine.files_epoch().unwrap();
    engine.files.as_mut().unwrap().vfs = Vfs32::new(
        epoch,
        FileLimits32 {
            max_io_bytes: 65_536,
            ..FileLimits32::default()
        },
    )
    .unwrap();
    let file = file(&mut engine, false);
    let id = {
        let files = engine.files.as_ref().unwrap();
        files
            .vfs
            .file_for_open(files.handles.file(file).unwrap())
            .unwrap()
    };
    let (_, view) = mapping(&mut engine, file, 131_072);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(view), &vec![13; 131_072])
        .unwrap();
    assert_eq!(
        complete(&mut engine, File::FlushViewOfFile.id(), &[view, 0]),
        1
    );
    let files = engine.files.as_mut().unwrap();
    let FileIo32::Ready(read) = files.vfs.prepare_read_at(id, 65_536, 65_536).unwrap() else {
        panic!("explicit flush owns its second source page")
    };
    assert!(read.bytes().iter().all(|value| *value == 13));
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(view), &vec![17; 131_072])
        .unwrap();
    complete(&mut engine, Kernel::ExitProcess.id(), &[0]);
    assert_eq!(engine.exit_code, Some(0));
    assert!(engine.files.as_ref().unwrap().reply.is_none());
    let files = engine.files.as_mut().unwrap();
    for offset in [0, 65_536] {
        let FileIo32::Ready(read) = files.vfs.prepare_read_at(id, offset, 65_536).unwrap() else {
            panic!("flushed overlay page is resident")
        };
        assert!(read.bytes().iter().all(|value| *value == 17));
    }
}
