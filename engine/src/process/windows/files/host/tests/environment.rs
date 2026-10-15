use super::*;
use crate::{
    abi::{
        callback::CallbackRecord32,
        x86::{EXIT_SIZE, STATE_SIZE},
    },
    cpu::x86::State32,
    process::{
        calls::{
            call::{PendingCall, PendingOwner},
            callback::{SuspendedCallback, SuspendedRecord},
        },
        cpu::context::ExtendedCpuState,
    },
    windows::{CallFrame32, CallingConvention32},
};
#[test]
fn large_environment_is_staged_atomically_with_stale_and_busy_guards() {
    let mut engine = engine();
    let epoch = engine.files_epoch().unwrap();
    for path in ["C:\\Windows", "C:\\Windows\\System32", "C:\\Temp"] {
        assert!(
            engine
                .files
                .as_ref()
                .unwrap()
                .vfs
                .path_info(&utf16(path))
                .is_ok()
        );
    }
    let mut environment = FileEnvironment32::default();
    environment.search_path = (0..100)
        .map(|index| utf16(&format!("C:\\search{index:03}{}", "x".repeat(50))))
        .collect();
    let paths = [
        &environment.application_directory,
        &environment.windows_directory,
        &environment.system_directory,
        &environment.temp_directory,
    ]
    .into_iter()
    .chain(environment.search_path.iter());
    let lengths: Vec<u32> = paths.clone().map(|p| p.len() as u32).collect();
    let units: Vec<u16> = paths.flat_map(|p| p.iter().copied()).collect();
    assert!(units.len() * 2 > 4096);
    let pending = engine.begin_file_environment(epoch, &lengths).unwrap();
    append(&mut engine, pending, &units);
    assert_eq!(
        engine.set_files_environment(epoch, environment.clone()),
        Err(HostError::File(FileError32::Busy))
    );
    assert_eq!(
        engine.files.as_ref().unwrap().file_environment().clone(),
        FileEnvironment32::default()
    );
    engine
        .finish_file_environment(epoch, pending.token)
        .unwrap();
    assert_eq!(
        engine.files.as_ref().unwrap().file_environment().clone(),
        environment
    );
    assert!(engine.pending_file_input().unwrap().is_none());
    assert_eq!(
        engine.finish_file_environment(epoch, pending.token),
        Err(HostError::File(FileError32::Stale))
    );
    let mut invalid = FileEnvironment32::default();
    invalid.temp_directory = utf16("relative");
    assert_eq!(
        engine.set_files_environment(epoch, invalid),
        Err(HostError::File(FileError32::InvalidName))
    );
    assert_eq!(
        engine.files.as_ref().unwrap().file_environment().clone(),
        environment
    );
}
#[test]
fn environment_changes_reject_captured_frames_and_suspended_callbacks() {
    let mut engine = engine();
    let epoch = engine.files_epoch().unwrap();
    engine.map(0x1000, 1, 3).unwrap();
    let mut state = State32::default();
    state.registers[4] = 0x1000;
    let frame = CallFrame32::capture(
        engine.memory.as_ref().unwrap(),
        state,
        CallingConvention32::Stdcall,
        0,
    )
    .unwrap();
    engine.pending_call = Some(PendingCall {
        token: 1,
        owner: PendingOwner::Replacement(1),
        frame,
        state: [0; STATE_SIZE],
        exit: [0; EXIT_SIZE],
        extended: ExtendedCpuState::initial().unwrap(),
    });
    let environment = FileEnvironment32::default();
    assert_eq!(
        engine.set_files_environment(epoch, environment.clone()),
        Err(HostError::File(FileError32::Busy))
    );
    let outer = engine.pending_call.take().unwrap();
    engine.callback = Some(SuspendedCallback {
        record: SuspendedRecord::Replacement(CallbackRecord32 {
            token: 2,
            outer_token: 1,
            phase: 1,
            outcome: 0,
            entry_pc: 0x2000,
            entry_esp: 0x1000,
            return_pc: 0x3000,
            return_id: 1,
            stack_words: 0,
            result: 0,
            generation: 1,
        }),
        outer,
        parent: None,
    });
    assert_eq!(
        engine.set_files_environment(epoch, environment),
        Err(HostError::File(FileError32::Busy))
    );
    assert_eq!(
        engine.files.as_ref().unwrap().file_environment().clone(),
        FileEnvironment32::default()
    );
}
