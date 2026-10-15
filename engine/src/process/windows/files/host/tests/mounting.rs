use super::*;
#[test]
fn staged_long_mount_checks_offsets_epoch_completion_and_late_tokens() {
    let mut engine = engine();
    let epoch = engine.files_epoch().unwrap();
    let path = long_path();
    let start = FileMountStart32 {
        units: path.len() as u32,
        asset: Some(AssetRef32 { id: 501, length: 0 }),
        attributes: 0x80,
        times: FileTimes32::default(),
    };
    assert_eq!(
        engine.begin_file_mount(epoch + 1, start),
        Err(HostError::File(FileError32::Stale))
    );
    let pending = engine.begin_file_mount(epoch, start).unwrap();
    assert_eq!(
        engine.finish_file_mount(epoch, pending.token),
        Err(HostError::File(FileError32::Busy))
    );
    assert_eq!(
        engine.append_file_input(epoch, pending.token, 1, &path[..2]),
        Err(HostError::File(FileError32::InvalidParameter))
    );
    assert_eq!(
        engine.append_file_input(epoch, pending.token + 1, 0, &path[..2]),
        Err(HostError::File(FileError32::Stale))
    );
    assert_eq!(
        engine.append_file_input(epoch, pending.token, 0, &vec![1; 2049]),
        Err(HostError::File(FileError32::InvalidParameter))
    );
    assert_eq!(engine.pending_file_input().unwrap().unwrap().received, 0);
    append(&mut engine, pending, &path);
    let file = engine.finish_file_mount(epoch, pending.token).unwrap();
    assert_eq!(
        engine.files.as_ref().unwrap().vfs.file_path(file).unwrap(),
        path[4..]
    );
    assert!(engine.pending_file_input().unwrap().is_none());
    assert_eq!(
        engine.append_file_input(epoch, pending.token, 0, &path[..1]),
        Err(HostError::File(FileError32::Stale))
    );
    let second = engine.begin_file_mount(epoch, start).unwrap();
    assert!(second.token > pending.token);
    engine.abort_file_input(epoch, second.token).unwrap();
    assert_eq!(
        engine.finish_file_mount(epoch, second.token),
        Err(HostError::File(FileError32::Stale))
    );
}
#[test]
fn invalid_finished_mount_remains_abortable_and_never_publishes_a_prefix() {
    let mut engine = engine();
    let epoch = engine.files_epoch().unwrap();
    let path = utf16("C:\\good\\bad|name");
    let start = FileMountStart32 {
        units: path.len() as u32,
        asset: None,
        attributes: 0,
        times: FileTimes32::default(),
    };
    let pending = engine.begin_file_mount(epoch, start).unwrap();
    append(&mut engine, pending, &path);
    let revision = engine.files.as_ref().unwrap().vfs.revision();
    assert_eq!(
        engine.finish_file_mount(epoch, pending.token),
        Err(HostError::File(FileError32::InvalidName))
    );
    assert_eq!(engine.files.as_ref().unwrap().vfs.revision(), revision);
    assert!(
        engine
            .files
            .as_ref()
            .unwrap()
            .vfs
            .path_info(&utf16("C:\\good"))
            .is_err()
    );
    engine.abort_file_input(epoch, pending.token).unwrap();
    assert!(engine.pending_file_input().unwrap().is_none());
}
