use super::*;
use crate::{
    abi::{
        arena::{EXIT_OFFSET, TRANSFER_OFFSET},
        x86::{EXIT_SIZE, STATE_SIZE, encode_exit_v3, encode_state_current},
    },
    cpu::{ExecutionExit, ExitReason, x86::State32},
    memory::GuestAddress,
    windows::WindowsApi32,
};

fn exit(engine: &mut EngineInstance) {
    const CODE: u32 = 0x2000;
    const STACK: u32 = 0x1000;
    engine.map(STACK, 1, 3).unwrap();
    engine.map(CODE, 1, 7).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(CODE), &[0x0f, 0x0b])
        .unwrap();
    for (index, word) in [CODE, 2, CODE, WindowsApi32::ExitProcess.id()]
        .iter()
        .enumerate()
    {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&word.to_le_bytes());
    }
    engine.compile_with_gates(1, 1).unwrap();
    for (index, word) in [CODE + 2, 23u32].iter().enumerate() {
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(STACK + index as u32 * 4), &word.to_le_bytes())
            .unwrap();
    }
    let mut state = State32 {
        eip: CODE,
        ..Default::default()
    };
    state.registers[4] = STACK;
    encode_state_current(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate {
                id: WindowsApi32::ExitProcess.id(),
            },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    let generation = engine.generation;
    let token = engine
        .capture_windows_call(0x7777, generation)
        .unwrap()
        .token;
    engine
        .complete_windows_call(0x7777, generation, token)
        .unwrap();
    assert_eq!(engine.exit_code, Some(23));
    assert!(matches!(engine.memory(), Err(HostError::ProcessExited)));
    assert!(engine.is_open());
}

#[test]
fn exit_keeps_lazy_save_export_available_until_close_without_reopening_execution() {
    let mut engine = engine();
    let epoch = engine.files_epoch().unwrap();
    let path = utf16("C:\\save.bin");
    let file = engine
        .mount_file(
            &path,
            AssetRef32 {
                id: 505,
                length: 10000,
            },
            0x80,
            FileTimes32::default(),
        )
        .unwrap();
    let files = engine.files.as_mut().unwrap();
    let attrs = files.vfs.prepare_set_attributes(&path, 2).unwrap();
    files.vfs.commit_namespace(attrs);
    exit(&mut engine);
    assert_eq!(engine.files_epoch().unwrap(), epoch);
    let entry = engine.file_overlay_entry(epoch, 0).unwrap().unwrap();
    assert_eq!(entry.info.unwrap().id, file);
    let mut path_output = [0; 32];
    let count = engine
        .read_file_overlay_path(epoch, entry.revision, 0, 0, &mut path_output)
        .unwrap();
    assert_eq!(&path_output[..count], path);
    let mut bytes = [0xa5; 32];
    assert_eq!(
        engine
            .read_file_overlay(epoch, file, entry.revision, 0, &mut bytes)
            .unwrap(),
        None
    );
    assert_eq!(bytes, [0xa5; 32]);
    let cancelled = engine.pending_file_request().unwrap().unwrap();
    engine.cancel_file_request(cancelled).unwrap();
    assert_eq!(
        engine.supply_file_bytes(cancelled, 0, &[8]),
        Err(HostError::File(FileError32::Stale))
    );
    assert_eq!(
        engine
            .read_file_overlay(epoch, file, entry.revision, 0, &mut bytes)
            .unwrap(),
        None
    );
    let request = engine.pending_file_request().unwrap().unwrap();
    assert_ne!(request.request, cancelled.request);
    for offset in (0..request.length).step_by(4096) {
        let count = (request.length - offset).min(4096) as usize;
        engine
            .supply_file_bytes(request, offset, &vec![0x39; count])
            .unwrap();
    }
    assert_eq!(
        engine
            .read_file_overlay(epoch, file, entry.revision, 0, &mut bytes)
            .unwrap(),
        Some(32)
    );
    assert_eq!(bytes, [0x39; 32]);
    assert_eq!(
        engine.progress_file_io(epoch),
        Err(HostError::ProcessExited)
    );
    assert_eq!(
        engine.configure_files(FileConfig32::default()),
        Err(HostError::ProcessExited)
    );
    assert_eq!(
        engine.mount_file(
            &utf16("C:\\new"),
            AssetRef32 { id: 506, length: 0 },
            0x80,
            FileTimes32::default()
        ),
        Err(HostError::ProcessExited)
    );
    engine
        .acknowledge_file_overlay(epoch, entry.revision)
        .unwrap();
    assert!(engine.file_overlay_entry(epoch, 0).unwrap().is_none());
    engine.close();
    assert_eq!(engine.files_epoch(), Err(HostError::Closed));
    assert_eq!(engine.pending_file_request(), Err(HostError::Closed));
    assert_eq!(
        engine.supply_file_bytes(request, u32::MAX, &[]),
        Err(HostError::Closed)
    );
    assert_eq!(engine.cancel_file_request(request), Err(HostError::Closed));
    assert_eq!(
        engine.file_overlay_entry(epoch + 1, usize::MAX),
        Err(HostError::Closed)
    );
    assert_eq!(
        engine.read_file_overlay_path(epoch + 1, u64::MAX, usize::MAX, u32::MAX, &mut path_output),
        Err(HostError::Closed)
    );
    let previous = bytes;
    assert_eq!(
        engine.read_file_overlay(epoch + 1, file, u64::MAX, u64::MAX, &mut bytes),
        Err(HostError::Closed)
    );
    assert_eq!(bytes, previous);
    assert_eq!(
        engine.acknowledge_file_overlay(epoch + 1, u64::MAX),
        Err(HostError::Closed)
    );
}
