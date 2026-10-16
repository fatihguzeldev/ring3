use crate::{
    abi::{arena::*, x86::*},
    cpu::{ExecutionExit, ExitReason},
    memory::GuestAddress,
    process::EngineInstance,
    windows::{CallingConvention32, console::ConsoleApi32},
};
fn call(engine: &mut EngineInstance, api: ConsoleApi32, args: &[u32]) -> u32 {
    for (index, word) in [0x10000u32, 2, 0x10000, api.id()].iter().enumerate() {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&word.to_le_bytes());
    }
    engine.compile_with_gates(1, 1).unwrap();
    let mut state = crate::cpu::x86::State32 {
        eip: 0x10000,
        ..Default::default()
    };
    state.registers[4] = 0x11800;
    let words = std::iter::once(0x10001)
        .chain(args.iter().copied())
        .flat_map(u32::to_le_bytes)
        .collect::<Vec<_>>();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x11800), &words)
        .unwrap();
    encode_state_current(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate { id: api.id() },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    let token = engine
        .capture_call(
            7,
            engine.generation,
            CallingConvention32::Stdcall,
            args.len() as u32,
        )
        .unwrap()
        .token;
    engine
        .complete_windows_call(7, engine.generation, token)
        .unwrap();
    decode_state(&engine.arena()[..STATE_SIZE])
        .unwrap()
        .registers[0]
}
#[test]
fn headless_console_handlers_are_real_metadata_and_file_handles_are_not_console_handles() {
    let mut engine = EngineInstance::new(4, 7).unwrap();
    engine.map(0x10000, 2, 7).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x10000), &[0x0f, 0x0b])
        .unwrap();
    engine.protect(0x10000, 1, 5).unwrap();
    engine.protect(0x11000, 1, 3).unwrap();
    assert_eq!(
        call(
            &mut engine,
            ConsoleApi32::SetConsoleCtrlHandler,
            &[0x10040, 1]
        ),
        1
    );
    assert_eq!(
        call(
            &mut engine,
            ConsoleApi32::SetConsoleCtrlHandler,
            &[0x10040, 0]
        ),
        1
    );
    assert_eq!(
        call(
            &mut engine,
            ConsoleApi32::SetConsoleCtrlHandler,
            &[0x10040, 0]
        ),
        0
    );
    assert_eq!(engine.windows_thread.last_error(), 87);
    engine.write32(0x11900, 0x12345678).unwrap();
    for api in [
        ConsoleApi32::VerifyConsoleIoHandle,
        ConsoleApi32::WriteConsoleA,
        ConsoleApi32::WriteConsoleW,
    ] {
        let args = if api == ConsoleApi32::VerifyConsoleIoHandle {
            vec![0x80000000]
        } else {
            vec![0x80000000, u32::MAX, u32::MAX, 0x11900, 0]
        };
        assert_eq!(call(&mut engine, api, &args), 0);
        assert_eq!(engine.windows_thread.last_error(), 6);
        let mut value = [0; 4];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(0x11900), &mut value)
            .unwrap();
        assert_eq!(u32::from_le_bytes(value), 0x12345678);
    }
}
