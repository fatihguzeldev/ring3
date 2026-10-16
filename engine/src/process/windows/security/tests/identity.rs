use super::*;
use crate::windows::process::ProcessApi32;

fn fixture() -> EngineInstance {
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
}

fn cases() -> [(u32, bool, &'static str, u32, u32); 4] {
    [
        (SecurityApi32::GetUserNameA.id(), false, "ring3", 6, 122),
        (SecurityApi32::GetUserNameW.id(), true, "ring3", 6, 122),
        (ProcessApi32::GetComputerNameA.id(), false, "RING3", 5, 111),
        (ProcessApi32::GetComputerNameW.id(), true, "RING3", 5, 111),
    ]
}

#[test]
fn captured_identity_names_keep_distinct_native_counts_and_ignore_short_output_addresses() {
    for (id, wide, name, copied, short_error) in cases() {
        let mut engine = fixture();
        engine.write32(0x11800, 5).unwrap();
        assert_eq!(complete(&mut engine, id, &[0xfffffff0, 0x11800]), 0);
        assert_eq!(word(&engine, 0x11800), 6);
        assert_eq!(engine.windows_thread.last_error(), short_error);
        engine.windows_thread = engine.windows_thread.with_last_error(0xabcdef01);
        assert_eq!(complete(&mut engine, id, &[0x11820, 0x11800]), 1);
        assert_eq!(word(&engine, 0x11800), copied);
        assert_eq!(engine.windows_thread.last_error(), 0xabcdef01);
        let expected: Vec<u8> = if wide {
            name.encode_utf16()
                .chain([0])
                .flat_map(u16::to_le_bytes)
                .collect()
        } else {
            name.bytes().chain([0]).collect()
        };
        let mut bytes = vec![0; expected.len()];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(0x11820), &mut bytes)
            .unwrap();
        assert_eq!(bytes, expected);
    }
}

#[test]
fn captured_identity_name_fault_keeps_the_count_and_partial_output_atomic() {
    for (id, _, _, copied, _) in cases() {
        let mut engine = fixture();
        engine.write32(0x11800, 6).unwrap();
        engine
            .memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(0x11ffe), &[0x5a; 2])
            .unwrap();
        engine.windows_thread = engine.windows_thread.with_last_error(0xabcdef01);
        let token = capture(&mut engine, id, &[0x11ffe, 0x11800]);
        let state = decode_state(&engine.arena()[..STATE_SIZE]).unwrap();
        assert!(matches!(
            engine.complete_windows_call(7, engine.generation, token),
            Err(crate::process::HostError::Memory(
                crate::memory::MemoryError::Fault(_)
            ))
        ));
        assert_eq!(decode_state(&engine.arena()[..STATE_SIZE]).unwrap(), state);
        assert_eq!(engine.pending_call.as_ref().unwrap().token, token);
        assert_eq!(word(&engine, 0x11800), 6);
        assert_eq!(engine.windows_thread.last_error(), 0xabcdef01);
        let mut bytes = [0; 2];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(0x11ffe), &mut bytes)
            .unwrap();
        assert_eq!(bytes, [0x5a; 2]);
        engine.map(0x12000, 1, 3).unwrap();
        engine
            .complete_windows_call(7, engine.generation, token)
            .unwrap();
        assert!(engine.pending_call.is_none());
        assert_eq!(
            decode_state(&engine.arena()[..STATE_SIZE])
                .unwrap()
                .registers[0],
            1
        );
        assert_eq!(word(&engine, 0x11800), copied);
    }
    assert!(!SecurityApi32::GetUserNameA.returns_status());
    assert!(!SecurityApi32::GetUserNameW.returns_status());
    assert!(SecurityApi32::GetSecurityInfo.returns_status());
}
