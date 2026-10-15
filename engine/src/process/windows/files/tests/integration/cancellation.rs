use super::*;
use crate::{
    files::{AssetRef32, FileIo32, FileTimes32},
    process::windows::files::tests::support,
    windows::files::FileApi32 as Api,
};

#[test]
fn parked_read_can_be_canceled_after_an_accepted_write_invalidates_its_fetch_receipt() {
    let mut engine = engine();
    let path: Vec<u16> = "C:\\immutable.bin".encode_utf16().collect();
    engine
        .mount_file(
            &path,
            AssetRef32 { id: 23, length: 1 },
            0x80,
            FileTimes32::default(),
        )
        .unwrap();
    support::path(engine.memory.as_mut().unwrap(), 0x1000, "C:\\immutable.bin");
    let handle = complete(
        &mut engine,
        Api::CreateFileW.id(),
        &[0x1000, 0xc000_0000, 7, 0, 3, 0x80, 0],
    );
    let token = capture(
        &mut engine,
        Api::ReadFile.id(),
        &[handle, 0x1300, 1, 0x1200, 0],
        None,
    );
    engine
        .complete_windows_call(KEY, engine.generation, token)
        .unwrap();
    let request = engine.pending_file_request().unwrap().unwrap();
    assert!(engine.pending_file_call().is_some());
    let files = engine.files.as_mut().unwrap();
    let file = files
        .vfs
        .file_for_open(files.handles.file(handle).unwrap())
        .unwrap();
    let write = match files
        .vfs
        .prepare_write_at(file, 0, &vec![9; 65_536])
        .unwrap()
    {
        FileIo32::Ready(write) => write,
        _ => panic!("full owned page needs no source bytes"),
    };
    files.vfs.commit_write(write);
    assert!(engine.supply_file_bytes(request, 0, &[1]).is_err());
    engine.cancel_file_call(KEY, token).unwrap();
    assert!(engine.pending_file_call().is_none());
    assert!(engine.pending_call.is_none());
    assert!(engine.resume_file_call(KEY, token).is_err());
}
