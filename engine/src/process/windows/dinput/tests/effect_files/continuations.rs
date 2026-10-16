use super::{
    super::{haptics::controller, support::DATA},
    marshaling::constant,
};
use crate::{
    abi::{
        arena::{EXIT_OFFSET, TRANSFER_OFFSET},
        x86::{EXIT_SIZE, STATE_SIZE, decode_state, encode_exit_v3, encode_state_current},
    },
    cpu::{ExecutionExit, ExitReason, x86::State32},
    files::{ATTR_NORMAL, AssetRef32, FileError32, FileTimes32},
    input::dinput::{InterfaceKind32, MethodGate, haptics::files::encode},
    memory::GuestAddress,
    process::{CallError, EngineInstance, FileConfig32, HostError, MediaConfig},
    windows::CallingConvention32,
};
const KEY: u64 = 77;
const CODE: u32 = 0xa000;
fn engine() -> (EngineInstance, GuestAddress) {
    let (runtime, memory, pointer, _) = controller();
    let mut engine = EngineInstance::new(512, KEY).unwrap();
    engine.memory = Some(memory);
    engine.dinput = Some(runtime);
    engine.configure_cpu_clock(1_000_000_000, 0).unwrap();
    engine.configure_media(MediaConfig::default()).unwrap();
    engine.configure_files(FileConfig32::default()).unwrap();
    engine.map(CODE, 1, 7).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(DATA), b"C:\\authored.ffe\0")
        .unwrap();
    (engine, pointer)
}
fn path() -> Vec<u16> {
    r"C:\authored.ffe".encode_utf16().collect()
}
fn capture(engine: &mut EngineInstance, device: GuestAddress) -> u32 {
    let id = MethodGate {
        interface: InterfaceKind32::Device8A,
        slot: 27,
    }
    .id();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(CODE), &[0x0f, 0x0b])
        .unwrap();
    for (i, word) in [CODE, 2, CODE, id].iter().enumerate() {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + i * 4..TRANSFER_OFFSET + i * 4 + 4]
            .copy_from_slice(&word.to_le_bytes());
    }
    engine.compile_with_gates(1, 1).unwrap();
    let stack = DATA + 4096;
    for (i, word) in [CODE + 128, device.0, DATA, 0x9000, 0x55, 0]
        .iter()
        .enumerate()
    {
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(stack + i as u32 * 4), &word.to_le_bytes())
            .unwrap();
    }
    let mut state = State32 {
        eip: CODE,
        ..Default::default()
    };
    state.registers[4] = stack;
    encode_state_current(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate { id },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    engine
        .capture_call(KEY, engine.generation, CallingConvention32::Stdcall, 5)
        .unwrap()
        .token
}
#[test]
fn lazy_effect_file_continuation_keeps_exact_request_ownership_and_cancel_rejects_old_bytes() {
    let (mut engine, device) = engine();
    let input = encode(&[constant(b"lazy")], 4096).unwrap();
    engine
        .mount_file(
            &path(),
            AssetRef32 {
                id: 31,
                length: input.len() as u64,
            },
            ATTR_NORMAL,
            FileTimes32::default(),
        )
        .unwrap();
    let token = capture(&mut engine, device);
    engine
        .complete_windows_call(KEY, engine.generation, token)
        .unwrap();
    assert_eq!(engine.pending_dinput_file_token(), Some(token));
    let request = engine.pending_file_request().unwrap().unwrap();
    assert_eq!(
        engine.abandon_call(KEY, token),
        Err(HostError::Call(CallError::Busy))
    );
    assert_eq!(
        engine.cancel_dinput_file_call(KEY, token + 1),
        Err(HostError::Call(CallError::InvalidToken))
    );
    let parked = engine.dinput.as_mut().unwrap().take_thread_reply();
    assert_eq!(parked.file_request(), Some(request));
    assert!(parked.file_owner().is_some());
    assert_eq!(engine.dinput.as_ref().unwrap().active_file_request(), None);
    engine.dinput.as_mut().unwrap().restore_thread_reply(parked);
    engine.cancel_dinput_file_call(KEY, token).unwrap();
    assert_eq!(
        engine.supply_file_bytes(request, 0, &input),
        Err(HostError::File(FileError32::Stale))
    );
    assert!(engine.pending_call.is_none());
    assert!(engine.pending_file_request().unwrap().is_none());
    assert!(engine.dinput.as_ref().unwrap().pending.is_empty());
}
#[test]
fn supplied_effect_bytes_finish_into_the_existing_directinput_callback_ledger() {
    let (mut engine, device) = engine();
    let input = encode(&[constant(b"supplied")], 4096).unwrap();
    engine
        .mount_file(
            &path(),
            AssetRef32 {
                id: 32,
                length: input.len() as u64,
            },
            ATTR_NORMAL,
            FileTimes32::default(),
        )
        .unwrap();
    let token = capture(&mut engine, device);
    engine
        .complete_windows_call(KEY, engine.generation, token)
        .unwrap();
    let request = engine.pending_file_request().unwrap().unwrap();
    engine.supply_file_bytes(request, 0, &input).unwrap();
    engine.resume_dinput_file_call(KEY, token).unwrap();
    assert_eq!(engine.pending_dinput_file_token(), None);
    let callback = engine.pending_windows_callback().unwrap();
    assert_eq!(callback.arguments[1], 0x55);
    let row = callback.arguments[0];
    let mut name = [0; 9];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(row + 24), &mut name)
        .unwrap();
    assert_eq!(&name, b"supplied\0");
    assert_eq!(
        engine.resume_dinput_file_call(KEY, token),
        Err(HostError::Call(CallError::InvalidToken))
    );
    assert_eq!(
        decode_state(&engine.arena()[..STATE_SIZE]).unwrap().eip,
        CODE
    );
}
