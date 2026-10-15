mod control;
mod locks;
mod transfer;

use crate::{
    files::{FileAccess32, FileDisposition32, FileOpenOptions32},
    memory::{AddressSpace, GuestAddress},
    process::windows::files::{FileRuntime32, ordinary::FileCallOutcome32, tests::support::*},
    windows::synchronization::SynchronizationContext32,
};
fn setup() -> (
    FileRuntime32,
    AddressSpace,
    SynchronizationContext32,
    u32,
    u32,
    u32,
) {
    let (mut files, mut memory, mut sync) = runtime();
    let file = open_options(
        &mut files,
        "C:\\async.bin",
        FileOpenOptions32 {
            access: FileAccess32::ALL,
            disposition: FileDisposition32::OpenAlways,
            overlapped: true,
            ..FileOpenOptions32::default()
        },
    );
    write_file(&mut files, file, &[1, 2, 3]);
    let (event, _) = sync.create_event(true, true, false, None).unwrap();
    let open = files.handles.file(file).unwrap();
    let file_event = files.file_event(open, &mut sync).unwrap();
    let mut overlap = [0; 20];
    overlap[16..].copy_from_slice(&event.to_le_bytes());
    memory.write(GuestAddress(0x1100), &overlap).unwrap();
    (files, memory, sync, file, event, file_event)
}
fn result(outcome: FileCallOutcome32) -> (u32, Option<u32>) {
    match outcome {
        FileCallOutcome32::Return { value, last_error } => (value, last_error),
        _ => panic!("expected completed API"),
    }
}
