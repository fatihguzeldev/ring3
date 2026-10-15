use super::*;
use crate::{
    files::FileIo32,
    process::windows::files::tests::support,
    windows::{WindowsApi32 as Api, files::FileApi32 as File},
};

#[test]
fn closing_one_alias_keeps_async_alive_and_final_numeric_close_reclaims_hidden_event() {
    let mut engine = engine_with_events(1);
    let handle = file(&mut engine, true);
    let files = engine.files.as_mut().unwrap();
    let id = files
        .vfs
        .file_for_open(files.handles.file(handle).unwrap())
        .unwrap();
    let write = match files.vfs.prepare_write_at(id, 0, &[7]).unwrap() {
        FileIo32::Ready(write) => write,
        _ => unreachable!(),
    };
    files.vfs.commit_write(write);
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1100), &[0; 20])
        .unwrap();
    assert_eq!(
        complete(
            &mut engine,
            File::ReadFile.id(),
            &[handle, 0x1300, 1, 0, 0x1100]
        ),
        0
    );
    assert_eq!(engine.windows_thread.last_error(), 997);
    assert_eq!(
        complete(
            &mut engine,
            Api::DuplicateHandle.id(),
            &[u32::MAX, handle, u32::MAX, 0x1200, 0, 0, 2]
        ),
        1
    );
    let alias = support::dword(engine.memory().unwrap(), 0x1200);
    assert_eq!(complete(&mut engine, Api::CloseHandle.id(), &[handle]), 1);
    let epoch = engine.files_epoch().unwrap();
    let progress = engine.progress_file_io(epoch).unwrap();
    assert_eq!((progress.error, progress.bytes), (0, 1));
    assert_eq!(complete(&mut engine, Api::CloseHandle.id(), &[alias]), 1);
    assert!(engine.files.as_ref().unwrap().events.is_empty());
    assert_ne!(
        complete(&mut engine, Api::CreateEventW.id(), &[0, 1, 0, 0]),
        0
    );
}

fn begin_copy(engine: &mut EngineInstance) -> (u64, [(crate::files::OpenFileId, u32); 2]) {
    let handle = file(engine, false);
    let files = engine.files.as_mut().unwrap();
    let id = files
        .vfs
        .file_for_open(files.handles.file(handle).unwrap())
        .unwrap();
    let write = match files.vfs.prepare_write_at(id, 0, &[7]).unwrap() {
        FileIo32::Ready(write) => write,
        _ => unreachable!(),
    };
    files.vfs.commit_write(write);
    support::path(
        engine.memory.as_mut().unwrap(),
        0x1400,
        "C:\\destination.bin",
    );
    let token = capture(
        engine,
        File::CopyFileExW.id(),
        &[0x1000, 0x1400, CODE + 0x200, 0, 0, 0],
        None,
    );
    engine
        .complete_windows_call(KEY, engine.generation, token)
        .unwrap();
    let continuation = engine.pending_windows_callback().unwrap().continuation;
    let closes = engine
        .files
        .as_ref()
        .unwrap()
        .pending_copy_closes(continuation)
        .unwrap();
    for &(open, _) in &closes {
        let files = engine.files.as_mut().unwrap();
        let sync = engine.synchronization.as_mut().unwrap().context_mut();
        files.file_event(open, sync).unwrap();
    }
    (continuation, closes)
}

#[test]
fn callback_terminal_cleanup_preflights_both_event_closes_before_native_handle_disposal() {
    use crate::{files::FileError32, process::HostError};
    let mut engine = engine_with_events(2);
    let (continuation, closes) = begin_copy(&mut engine);
    let revision = engine.synchronization().unwrap().revision();
    engine
        .synchronization
        .as_mut()
        .unwrap()
        .context_mut()
        .set_revision_for_test(u64::MAX - 1);
    assert!(matches!(
        engine.prepare_file_callback_cleanup(continuation),
        Err(HostError::File(FileError32::Exhausted))
    ));
    for &(open, _) in &closes {
        assert_eq!(engine.files.as_ref().unwrap().handles.file_count(open), 1);
    }
    assert_eq!(engine.files.as_ref().unwrap().events.len(), 2);
    engine
        .synchronization
        .as_mut()
        .unwrap()
        .context_mut()
        .set_revision_for_test(revision);
    let cleanup = engine.prepare_file_callback_cleanup(continuation).unwrap();
    let outcome = engine
        .files
        .as_mut()
        .unwrap()
        .complete_file_callback(engine.memory.as_mut().unwrap(), continuation, 2)
        .unwrap();
    assert!(matches!(
        outcome,
        crate::process::windows::files::FileCallOutcome32::Return { .. }
    ));
    engine.commit_file_callback_cleanup(cleanup);
    assert!(engine.files.as_ref().unwrap().events.is_empty());
    engine
        .synchronization
        .as_mut()
        .unwrap()
        .context_mut()
        .create_event(false, false, false, None)
        .unwrap();
}

#[test]
fn callback_cleanup_keeps_hidden_event_when_guest_retained_a_duplicate_temp_handle() {
    use crate::process::windows::files::handles::KernelFileObject32;
    let mut engine = engine_with_events(2);
    let (continuation, closes) = begin_copy(&mut engine);
    let source = closes[0].0;
    let files = engine.files.as_mut().unwrap();
    files.vfs.retain_open(source).unwrap();
    let prepared = files.handles.prepare(0x0010_0011, false).unwrap();
    files
        .handles
        .commit(prepared, KernelFileObject32::File(source));
    let cleanup = engine.prepare_file_callback_cleanup(continuation).unwrap();
    engine
        .files
        .as_mut()
        .unwrap()
        .complete_file_callback(engine.memory.as_mut().unwrap(), continuation, 2)
        .unwrap();
    engine.commit_file_callback_cleanup(cleanup);
    let files = engine.files.as_ref().unwrap();
    assert_eq!(files.handles.file_count(source), 1);
    assert_eq!(files.events.len(), 1);
    assert_eq!(files.events[0].0, source);
    assert!(
        engine
            .synchronization()
            .unwrap()
            .event_is_signaled(files.events[0].1)
            .unwrap()
    );
}
