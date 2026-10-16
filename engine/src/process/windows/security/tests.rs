use crate::{
    abi::{arena::*, x86::*},
    cpu::{ExecutionExit, ExitReason, x86::State32},
    memory::GuestAddress,
    process::EngineInstance,
    windows::{CallingConvention32, security::SecurityApi32},
};

#[test]
fn captured_security_descriptor_validation_and_length_preserve_last_error_on_success() {
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
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x11800), &[1, 0, 4, 0x80])
        .unwrap();
    for api in [
        SecurityApi32::IsValidSecurityDescriptor,
        SecurityApi32::GetSecurityDescriptorLength,
    ] {
        for (index, word) in [0x10000u32, 2, 0x10000, api.id()].into_iter().enumerate() {
            engine.arena_mut().unwrap()
                [TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
                .copy_from_slice(&word.to_le_bytes());
        }
        engine.compile_with_gates(1, 1).unwrap();
        engine.write32(0x11900, 0x10002).unwrap();
        engine.write32(0x11904, 0x11800).unwrap();
        let mut state = State32 {
            eip: 0x10000,
            ..Default::default()
        };
        state.registers[4] = 0x11900;
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
            .capture_call(7, engine.generation, CallingConvention32::Stdcall, 1)
            .unwrap()
            .token;
        engine.windows_thread = engine.windows_thread.with_last_error(0x1234);
        engine
            .complete_windows_call(7, engine.generation, token)
            .unwrap();
        assert_eq!(
            decode_state(&engine.arena()[..STATE_SIZE])
                .unwrap()
                .registers[0],
            if api == SecurityApi32::IsValidSecurityDescriptor {
                1
            } else {
                20
            }
        );
        assert_eq!(engine.windows_thread.last_error(), 0x1234);
    }
}

fn complete(engine: &mut EngineInstance, id: u32, arguments: &[u32]) -> u32 {
    for (index, word) in [0x10000, 2, 0x10000, id].into_iter().enumerate() {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&word.to_le_bytes());
    }
    engine.compile_with_gates(1, 1).unwrap();
    for (index, word) in std::iter::once(0x10002u32)
        .chain(arguments.iter().copied())
        .enumerate()
    {
        engine.write32(0x11900 + index as u32 * 4, word).unwrap();
    }
    let mut state = State32 {
        eip: 0x10000,
        ..Default::default()
    };
    state.registers[4] = 0x11900;
    encode_state_current(&state, &mut engine.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate { id },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    let token = engine
        .capture_windows_call(7, engine.generation)
        .unwrap()
        .token;
    engine
        .complete_windows_call(7, engine.generation, token)
        .unwrap();
    assert!(engine.pending_call.is_none());
    decode_state(&engine.arena()[..STATE_SIZE])
        .unwrap()
        .registers[0]
}
fn word(engine: &EngineInstance, address: u32) -> u32 {
    let mut bytes = [0; 4];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(address), &mut bytes)
        .unwrap();
    u32::from_le_bytes(bytes)
}

#[test]
fn captured_security_info_returns_owned_local_buffer_and_status_errors_preserve_last_error() {
    use crate::windows::{files::FileApi32, memory::MemoryApi32};
    let mut engine = EngineInstance::new(32, 7).unwrap();
    engine.map(0x10000, 1, 7).unwrap();
    engine.map(0x11000, 1, 3).unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x10000), &[0x0f, 0x0b])
        .unwrap();
    engine
        .configure_files(crate::process::FileConfig32::default())
        .unwrap();
    let path = "C:\\security-info"
        .encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x11800), &path)
        .unwrap();
    let handle = complete(
        &mut engine,
        FileApi32::CreateFileW.id(),
        &[0x11800, 0x10000000, 7, 0, 1, 0, 0],
    );
    assert_ne!(handle, u32::MAX);
    engine.windows_thread = engine.windows_thread.with_last_error(0x1234);
    assert_eq!(
        complete(
            &mut engine,
            SecurityApi32::GetSecurityInfo.id(),
            &[handle, 1, 7, 0x11880, 0x11884, 0x11888, 0, 0x1188c]
        ),
        0
    );
    let block = word(&engine, 0x1188c);
    assert_ne!(block, 0);
    assert_eq!(word(&engine, 0x11880), block + 20);
    assert_eq!(
        word(&engine, 0x11884),
        block + 20 + engine.windows_security.token().user().byte_len() as u32
    );
    assert!(word(&engine, 0x11888) > word(&engine, 0x11884));
    assert!(engine.check_windows_runtime_bytes(block, 20).is_ok());
    assert_eq!(engine.windows_thread.last_error(), 0x1234);
    assert_eq!(
        complete(&mut engine, MemoryApi32::LocalFree.id(), &[block]),
        0
    );
    assert!(engine.check_windows_runtime_bytes(block, 1).is_err());
    assert_eq!(
        complete(
            &mut engine,
            SecurityApi32::GetSecurityInfo.id(),
            &[handle, 1, 7, 0x11880, 0, 0, 0, 0]
        ),
        87
    );
    assert_eq!(
        complete(
            &mut engine,
            SecurityApi32::GetSecurityInfo.id(),
            &[handle, 1, 7, 0, 0, 0, 0, 0x9000]
        ),
        998
    );
    assert_eq!(engine.windows_thread.last_error(), 0x1234);
    assert_eq!(
        complete(
            &mut engine,
            SecurityApi32::SetSecurityInfo.id(),
            &[handle, 1, 4, 0, 0, 0, 0]
        ),
        0
    );
    assert_eq!(
        complete(
            &mut engine,
            SecurityApi32::GetNamedSecurityInfoW.id(),
            &[0x11800, 1, 4, 0, 0, 0x11888, 0, 0x1188c]
        ),
        0
    );
    assert_eq!(word(&engine, 0x11888), 0);
    let block = word(&engine, 0x1188c);
    assert_eq!(
        complete(&mut engine, MemoryApi32::LocalFree.id(), &[block]),
        0
    );
    assert_eq!(
        complete(
            &mut engine,
            SecurityApi32::GetSecurityInfo.id(),
            &[handle, 1, 8, 0, 0, 0, 0, 0]
        ),
        1314
    );
    assert_eq!(engine.windows_thread.last_error(), 0x1234);
}
