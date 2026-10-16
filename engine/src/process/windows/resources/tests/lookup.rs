use super::*;
#[test]
fn find_load_size_lock_and_loadstring_use_real_module_memory() {
    let mut engine = engine(false, 8);
    let resource = value(&mut engine, Api::FindResourceW, &[0, 7, 10]).unwrap();
    assert_eq!(
        value(&mut engine, Api::FindResourceW, &[0, 7, 10]),
        Ok(resource)
    );
    assert_eq!(
        value(&mut engine, Api::SizeofResource, &[0, resource]),
        Ok(4)
    );
    let data = value(&mut engine, Api::LoadResource, &[0, resource]).unwrap();
    assert_eq!(data, BASE + 0x2180);
    assert_eq!(value(&mut engine, Api::LockResource, &[data]), Ok(data));
    assert_eq!(read::<4>(&engine, data), [0x11, 0x22, 0x33, 0x44]);
    assert_eq!(
        value(&mut engine, Api::LoadStringW, &[0, 0, BASE + 0x2300, 4]),
        Ok(3)
    );
    assert_eq!(
        read::<8>(&engine, BASE + 0x2300),
        [b'h', 0, b'e', 0, b'l', 0, 0, 0]
    );
    assert_eq!(
        value(&mut engine, Api::LoadStringW, &[0, 0, BASE + 0x2300, 0]),
        Ok(5)
    );
    assert_eq!(
        u32::from_le_bytes(read::<4>(&engine, BASE + 0x2300)),
        BASE + 0x2102
    );
    assert_eq!(
        value(&mut engine, Api::LoadStringA, &[0, 0, BASE + 0x2300, 8]),
        Ok(5)
    );
    assert_eq!(read::<6>(&engine, BASE + 0x2300), *b"hello\0");
    assert_eq!(value(&mut engine, Api::FreeResource, &[data]), Ok(0));
    assert_eq!(value(&mut engine, Api::LockResource, &[data]), Ok(data));
}
#[test]
fn bad_string_output_is_neutral_and_old_resource_identity_does_not_alias_reloaded_dll() {
    let mut engine = engine(false, 1);
    let version = engine.memory.as_ref().unwrap().transaction_version();
    assert!(value(&mut engine, Api::LoadStringW, &[0, 0, 0xfffffff0, 0]).is_err());
    assert_eq!(
        engine.memory.as_ref().unwrap().transaction_version(),
        version
    );
    register(&mut engine, "resource.dll", &image(0x500000, true, false));
    assert!(matches!(
        engine.load_library_module("resource.dll", None),
        Ok(ModuleOperation32::Loaded { handle: 0x500000 })
    ));
    let old = value(&mut engine, Api::FindResourceW, &[0x500000, 7, 10]).unwrap();
    let old_data = value(&mut engine, Api::LoadResource, &[0x500000, old]).unwrap();
    assert_eq!(
        engine.free_library_module(0x500000),
        Ok(ModuleOperation32::Released)
    );
    assert!(matches!(
        engine.load_library_module("resource.dll", None),
        Ok(ModuleOperation32::Loaded { handle: 0x500000 })
    ));
    assert_eq!(
        value(&mut engine, Api::SizeofResource, &[0x500000, old]),
        Err(ResourceError32::InvalidHandle)
    );
    assert_eq!(
        value(&mut engine, Api::LockResource, &[old_data]),
        Err(ResourceError32::InvalidHandle)
    );
    let new = value(&mut engine, Api::FindResourceW, &[0x500000, 7, 10]).unwrap();
    assert_ne!(new, old);
}

fn captured_resource_call(engine: &mut EngineInstance, api: Api, args: &[u32]) -> u32 {
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
fn captured_loadstring_ansi_uses_configured_page_for_authored_unicode_resource() {
    use crate::{
        memory::Permissions,
        windows::{nls::NlsProfile32, text::CodePage32},
    };
    let mut bytes = image(BASE, false, false);
    bytes[0x502..0x504].copy_from_slice(&0x0131u16.to_le_bytes());
    let mut engine = engine_with_image(
        bytes,
        8,
        NlsProfile32 {
            locale: 0x41f,
            ansi: CodePage32::from_id(1254).unwrap(),
            oem: CodePage32::from_id(857).unwrap(),
            ..Default::default()
        },
    );
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
    assert_eq!(
        captured_resource_call(&mut engine, Api::LoadStringW, &[0, 0, BASE + 0x2300, 16]),
        5
    );
    assert_eq!(
        read::<12>(&engine, BASE + 0x2300),
        [0x31, 0x01, b'e', 0, b'l', 0, b'l', 0, b'o', 0, 0, 0]
    );
    assert_eq!(
        captured_resource_call(&mut engine, Api::LoadStringA, &[0, 0, BASE + 0x2300, 16]),
        5
    );
    assert_eq!(
        read::<6>(&engine, BASE + 0x2300),
        [0xfd, b'e', b'l', b'l', b'o', 0]
    );
}
