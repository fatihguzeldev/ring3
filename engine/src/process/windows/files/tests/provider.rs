use crate::{
    files::{ATTR_NORMAL, AssetRef32, FileError32, FileIo32, FileTimes32},
    process::windows::files::FileConfig32,
    process::{EngineInstance, HostError},
};

fn engine(operations: usize, overlay_pages: usize) -> EngineInstance {
    let mut engine = EngineInstance::new(128, 7).unwrap();
    engine
        .configure_files(FileConfig32 {
            operations,
            vfs: crate::files::FileLimits32 {
                overlay_pages,
                ..Default::default()
            },
            ..Default::default()
        })
        .unwrap();
    engine
        .files
        .as_mut()
        .unwrap()
        .vfs
        .mount_asset(
            &path(),
            AssetRef32 {
                id: 19,
                length: 200,
            },
            ATTR_NORMAL,
            FileTimes32::default(),
        )
        .unwrap();
    engine
}
fn path() -> Vec<u16> {
    r"C:\effects.ffe".encode_utf16().collect()
}

#[test]
fn provider_acl_changes_reject_new_reads_and_atomic_replacements_but_preserve_existing_grants() {
    use crate::windows::security::{Acl32, Dacl32};
    let mut engine = engine(4, 8);
    let lease = engine.open_provider_file(&path(), false).unwrap();
    let token = engine.windows_security.token().clone();
    let files = engine.files.as_mut().unwrap();
    let id = files.vfs.path_info(&path()).unwrap().id;
    let mut descriptor = files.vfs.file_security(id, 4, &token, None).unwrap();
    descriptor.dacl = Dacl32::Acl(Acl32 {
        revision: 2,
        aces: Vec::new(),
    });
    let prepared = files
        .vfs
        .prepare_file_security(id, 4, descriptor, &token, None)
        .unwrap();
    files.vfs.commit_file_security(prepared);
    let version = files.vfs.revision();
    assert_eq!(
        engine.open_provider_file(&path(), false),
        Err(HostError::File(FileError32::AccessDenied))
    );
    assert_eq!(
        engine.replace_provider_file(&path(), b"forbidden"),
        Err(HostError::File(FileError32::AccessDenied))
    );
    assert_eq!(engine.files.as_ref().unwrap().vfs.revision(), version);
    assert_eq!(engine.provider_file_size(lease).unwrap(), 200);
    let FileIo32::NeedBytes(request) = engine.read_provider_file(lease, 0, 200).unwrap() else {
        panic!("retained open grant")
    };
    engine.supply_file_bytes(request, 0, &[42; 200]).unwrap();
    assert!(
        matches!(engine.read_provider_file(lease,0,200),Ok(FileIo32::Ready(bytes)) if bytes==[42;200])
    );
    engine.close_provider_file(lease).unwrap();
}

#[test]
fn provider_metadata_and_group_cleanup_keep_exact_paths_and_do_not_partially_close() {
    let mut engine = engine(4, 8);
    let a = engine.open_provider_file(&path(), false).unwrap();
    let b = engine.open_provider_file(&path(), false).unwrap();
    assert_eq!(engine.provider_file_path(a).unwrap(), path());
    assert_eq!(
        engine
            .find_provider_paths(&r"C:\*.ffe".encode_utf16().collect::<Vec<_>>(), 1)
            .unwrap(),
        [path()]
    );
    let version = engine.files.as_ref().unwrap().vfs.revision();
    assert!(engine.check_close_provider_files(&[a, a]).is_err());
    assert_eq!(engine.files.as_ref().unwrap().vfs.revision(), version);
    assert_eq!(engine.provider_file_size(a).unwrap(), 200);
    engine.check_close_provider_files(&[a, b]).unwrap();
    engine.commit_close_provider_files(&[a, b]);
    assert!(engine.provider_file_size(a).is_err());
    assert!(engine.provider_file_path(b).is_err());
    engine
        .files
        .as_mut()
        .unwrap()
        .vfs
        .mount_asset(
            &r"C:\other.ffe".encode_utf16().collect::<Vec<_>>(),
            AssetRef32 { id: 20, length: 0 },
            ATTR_NORMAL,
            FileTimes32::default(),
        )
        .unwrap();
    assert_eq!(
        engine.find_provider_paths(&r"C:\*.ffe".encode_utf16().collect::<Vec<_>>(), 1),
        Err(HostError::File(FileError32::Capacity))
    );
}

#[test]
fn independent_provider_fetches_keep_cancel_authority_with_their_file_lease() {
    let mut engine = engine(4, 8);
    let first = engine.open_provider_file(&path(), false).unwrap();
    let second = engine.open_provider_file(&path(), false).unwrap();
    let FileIo32::NeedBytes(cancelled) = engine.read_provider_file(first, 0, 200).unwrap() else {
        panic!()
    };
    let FileIo32::NeedBytes(pending) = engine.read_provider_file(second, 0, 200).unwrap() else {
        panic!()
    };
    assert_ne!(cancelled.request, pending.request);
    engine.close_provider_file(first).unwrap();
    assert_eq!(
        engine.supply_file_bytes(cancelled, 0, &[1]),
        Err(HostError::File(FileError32::Stale))
    );
    engine.supply_file_bytes(pending, 0, &[42; 200]).unwrap();
    assert!(engine.provider_file_request_ready(pending));
    let FileIo32::Ready(bytes) = engine.read_provider_file(second, 0, 200).unwrap() else {
        panic!()
    };
    assert_eq!(bytes, vec![42; 200]);
    engine.close_provider_file(second).unwrap();
    assert!(engine.provider_file_size(second).is_err());
    assert!(engine.pending_file_request().unwrap().is_none());
}

#[test]
fn provider_thread_exit_closes_leases_and_cancels_fetches_in_the_existing_transaction() {
    let mut engine = engine(4, 8);
    let lease = engine.open_provider_file(&path(), false).unwrap();
    let FileIo32::NeedBytes(pending) = engine.read_provider_file(lease, 0, 200).unwrap() else {
        panic!()
    };
    let tid = engine.current_windows_thread_id().unwrap();
    let files = engine.files.as_ref().unwrap();
    let prepared = files
        .prepare_thread_exit(engine.memory().unwrap(), tid, &[], &[], false)
        .unwrap();
    engine
        .files
        .as_mut()
        .unwrap()
        .commit_thread_exit(tid, prepared);
    assert!(engine.provider_file_size(lease).is_err());
    assert_eq!(
        engine.supply_file_bytes(pending, 0, &[1]),
        Err(HostError::File(FileError32::Stale))
    );
    assert!(engine.pending_file_request().unwrap().is_none());
}

#[test]
fn complete_file_replacement_is_neutral_on_capacity_failure_and_needs_no_original_bytes() {
    let mut engine = engine(4, 1);
    assert_eq!(
        engine.replace_provider_file(&path(), &vec![9; 65_537]),
        Err(HostError::File(FileError32::DiskFull))
    );
    let lease = engine.open_provider_file(&path(), false).unwrap();
    assert_eq!(engine.provider_file_size(lease).unwrap(), 200);
    engine.close_provider_file(lease).unwrap();
    assert_eq!(engine.replace_provider_file(&path(), b"RIFF-owned"), Ok(10));
    assert!(engine.pending_file_request().unwrap().is_none());
    let lease = engine.open_provider_file(&path(), false).unwrap();
    assert_eq!(engine.provider_file_size(lease).unwrap(), 10);
    let FileIo32::Ready(bytes) = engine.read_provider_file(lease, 0, 200).unwrap() else {
        panic!()
    };
    assert_eq!(bytes, b"RIFF-owned");
    engine.close_provider_file(lease).unwrap();
}
