use super::*;
use crate::memory::GuestAddress;

#[test]
fn ansi_environment_snapshots_keep_their_contents_and_require_the_matching_free_api() {
    let (mut engine, _) = super::super::module_name_tests::engine();
    engine
        .process_runtime
        .environment
        .set("R3_CRT", Some("Grüße"))
        .unwrap();
    let first = engine
        .execute_process_call(api("GetEnvironmentStringsA"), &[])
        .unwrap()
        .value as u32;
    let mut bytes = [0; 15];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(first), &mut bytes)
        .unwrap();
    assert_eq!(&bytes, b"R3_CRT=Gr\x81\xe1e\0\0\0");
    engine
        .process_runtime
        .environment
        .set("R3_CRT", Some("changed"))
        .unwrap();
    let second = engine
        .execute_process_call(api("GetEnvironmentStringsA"), &[])
        .unwrap()
        .value as u32;
    assert_ne!(first, second);
    let mut retained = [0; 14];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(first), &mut retained)
        .unwrap();
    assert_eq!(&retained, b"R3_CRT=Gr\x81\xe1e\0\0");
    assert!(matches!(
        engine.execute_process_call(api("FreeEnvironmentStringsW"), &[first]),
        Err(HostError::WindowsProcess(ProcessError32::InvalidParameter))
    ));
    assert_eq!(
        engine
            .execute_process_call(api("FreeEnvironmentStringsA"), &[first])
            .unwrap()
            .value,
        1
    );
    assert!(
        engine
            .execute_process_call(api("FreeEnvironmentStringsA"), &[first])
            .is_err()
    );
    assert_eq!(
        engine
            .execute_process_call(api("FreeEnvironmentStringsA"), &[second])
            .unwrap()
            .value,
        1
    );
}

fn captured_environment_call(engine: &mut EngineInstance, api: ProcessApi32, args: &[u32]) -> u32 {
    use crate::{
        abi::{arena::*, x86::*},
        cpu::{ExecutionExit, ExitReason, x86::State32},
    };
    const CODE: u32 = 0x70000;
    const STACK: u32 = 0x71000;
    for (index, word) in [CODE, 2, CODE, api.id()].iter().enumerate() {
        engine.arena_mut().unwrap()[TRANSFER_OFFSET + index * 4..TRANSFER_OFFSET + index * 4 + 4]
            .copy_from_slice(&word.to_le_bytes());
    }
    let generation = engine.compile_with_gates(1, 1).unwrap();
    for (index, word) in std::iter::once(CODE + 0x80)
        .chain(args.iter().copied())
        .enumerate()
    {
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
    encode_state_current(
        &state,
        &mut engine.arena_mut().unwrap()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE],
    )
    .unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate { id: api.id() },
        },
        &mut engine.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    let token = engine.capture_windows_call(7, generation).unwrap().token;
    engine.complete_windows_call(7, generation, token).unwrap();
    assert!(engine.pending_call.is_none());
    let state = decode_state(&engine.arena()[STATE_OFFSET..STATE_OFFSET + STATE_SIZE]).unwrap();
    assert_eq!(
        (state.eip, state.registers[4]),
        (CODE + 0x80, STACK + 4 + args.len() as u32 * 4)
    );
    state.registers[0]
}

#[test]
fn captured_ansi_environment_uses_configured_page_for_names_and_both_value_directions() {
    use crate::{
        memory::{PageRange, Permissions},
        windows::{nls::NlsProfile32, text::CodePage32},
    };
    let (mut engine, _) = super::super::module_name_tests::engine_with_profile(NlsProfile32 {
        locale: 0x41f,
        ansi: CodePage32::from_id(1254).unwrap(),
        oem: CodePage32::from_id(857).unwrap(),
        ..Default::default()
    });
    engine
        .memory
        .as_mut()
        .unwrap()
        .map_zeroed(
            PageRange::new(GuestAddress(0x70000), 2).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    engine
        .memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x70000), &[0x0f, 0x0b])
        .unwrap();
    for (address, bytes) in [
        (
            0x30100,
            "R3_ı\0"
                .encode_utf16()
                .flat_map(u16::to_le_bytes)
                .collect::<Vec<_>>(),
        ),
        (
            0x30140,
            "ı!\0".encode_utf16().flat_map(u16::to_le_bytes).collect(),
        ),
        (0x30180, b"R3_\xfd\0".to_vec()),
        (0x301c0, b"\xfd?\0".to_vec()),
    ] {
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(address), &bytes)
            .unwrap();
    }
    assert_eq!(
        captured_environment_call(
            &mut engine,
            api("SetEnvironmentVariableW"),
            &[0x30100, 0x30140]
        ),
        1
    );
    assert_eq!(
        captured_environment_call(
            &mut engine,
            api("GetEnvironmentVariableA"),
            &[0x30180, 0x30300, 16]
        ),
        2
    );
    let mut ansi = [0; 3];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x30300), &mut ansi)
        .unwrap();
    assert_eq!(ansi, [0xfd, b'!', 0]);
    assert_eq!(
        captured_environment_call(
            &mut engine,
            api("SetEnvironmentVariableA"),
            &[0x30180, 0x301c0]
        ),
        1
    );
    assert_eq!(
        captured_environment_call(
            &mut engine,
            api("GetEnvironmentVariableW"),
            &[0x30100, 0x30300, 16]
        ),
        2
    );
    let mut wide = [0; 6];
    engine
        .memory()
        .unwrap()
        .read(GuestAddress(0x30300), &mut wide)
        .unwrap();
    assert_eq!(wide, [0x31, 0x01, b'?', 0, 0, 0]);
}
