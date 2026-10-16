use crate::{
    abi::{
        arena::{EXIT_OFFSET, TRANSFER_OFFSET},
        x86::{EXIT_SIZE, STATE_SIZE, decode_state, encode_exit_v3, encode_state_current},
    },
    cpu::{ExecutionExit, ExitReason, x86::State32},
    memory::GuestAddress,
    process::{EngineInstance, HostError},
    windows::{nls::NlsProfile32, shlwapi::ShlwapiApi32 as Api, text::CodePage32},
};
const KEY: u64 = 0x5a41;
fn engine() -> EngineInstance {
    let mut e = EngineInstance::new(64, KEY).unwrap();
    e.map(0x1000, 2, 3).unwrap();
    e.map(0x3000, 1, 7).unwrap();
    e.memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x3000), &[0x0f, 0x0b])
        .unwrap();
    e
}
fn captured(e: &mut EngineInstance, id: u32, args: &[u32]) -> Result<u32, HostError> {
    for (i, x) in [0x3000u32, 2, 0x3000, id].into_iter().enumerate() {
        e.arena_mut().unwrap()[TRANSFER_OFFSET + i * 4..TRANSFER_OFFSET + i * 4 + 4]
            .copy_from_slice(&x.to_le_bytes());
    }
    e.compile_with_gates(1, 1).unwrap();
    for (i, x) in std::iter::once(0x3002)
        .chain(args.iter().copied())
        .enumerate()
    {
        e.memory
            .as_mut()
            .unwrap()
            .write(GuestAddress(0x1000 + i as u32 * 4), &x.to_le_bytes())
            .unwrap();
    }
    let mut state = State32 {
        eip: 0x3000,
        ..Default::default()
    };
    state.registers[4] = 0x1000;
    encode_state_current(&state, &mut e.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate { id: id },
        },
        &mut e.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    let generation = e.generation;
    let token = e.capture_windows_call(KEY, generation).unwrap().token;
    e.complete_windows_call(KEY, generation, token)?;
    Ok(decode_state(&e.arena()[..STATE_SIZE]).unwrap().registers[0])
}
fn try_call(e: &mut EngineInstance, api: Api, args: &[u32]) -> Result<u32, HostError> {
    captured(e, api.id(), args)
}
fn nls_call(e: &mut EngineInstance, api: crate::windows::nls::NlsApi32, args: &[u32]) -> u32 {
    captured(e, api.id(), args).unwrap()
}
fn call(e: &mut EngineInstance, api: Api, args: &[u32]) -> u32 {
    try_call(e, api, args).unwrap()
}
fn wide(e: &mut EngineInstance, at: u32, value: &str) {
    let bytes = value
        .encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect::<Vec<_>>();
    e.memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(at), &bytes)
        .unwrap();
}
#[test]
fn captured_path_combine_publishes_canonical_path_and_exact_result_pointer() {
    let mut e = engine();
    wide(&mut e, 0x1200, r"C:\one\two");
    wide(&mut e, 0x1300, r"..\three");
    assert_eq!(
        call(&mut e, Api::PathCombineW, &[0x1400, 0x1200, 0x1300]),
        0x1400
    );
    let mut bytes = [0; 26];
    e.memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(0x1400), &mut bytes)
        .unwrap();
    assert_eq!(
        &bytes,
        &r"C:\one\three"
            .encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect::<Vec<_>>()[..]
    );
}
#[test]
fn captured_ansi_pointer_helpers_do_not_split_dbcs_trailing_backslash_bytes() {
    let mut e = engine();
    e.configure_nls(NlsProfile32 {
        locale: 0x411,
        ansi: CodePage32::from_id(932).unwrap(),
        oem: CodePage32::from_id(932).unwrap(),
        ..Default::default()
    })
    .unwrap();
    e.memory
        .as_mut()
        .unwrap()
        .write(
            GuestAddress(0x1200),
            &[b'C', b':', b'\\', 0x83, 0x5c, b'\\', b'x', b'.', b'a', 0],
        )
        .unwrap();
    assert_eq!(call(&mut e, Api::PathFindFileNameA, &[0x1200]), 0x1206);
    assert_eq!(call(&mut e, Api::PathFindExtensionA, &[0x1200]), 0x1207);
}

#[test]
fn remove_backslash_returns_the_replaced_null_or_the_original_final_character() {
    let mut e = engine();
    wide(&mut e, 0x1200, r"C:\folder\");
    assert_eq!(call(&mut e, Api::PathRemoveBackslashW, &[0x1200]), 0x1212);
    assert_eq!(call(&mut e, Api::PathRemoveBackslashW, &[0x1200]), 0x1210);
    let mut end = [0xff; 2];
    e.memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(0x1212), &mut end)
        .unwrap();
    assert_eq!(end, [0; 2]);
}

#[test]
fn zero_count_ansi_comparison_does_not_read_invalid_guest_pointers() {
    let mut e = engine();
    assert_eq!(
        call(&mut e, Api::StrCmpNIA, &[0xfffffff0, 0xffffffe0, 0]),
        0
    );
}
#[test]
fn native_wide_comparison_reads_terminated_inputs_before_applying_the_count() {
    let mut e = engine();
    assert!(matches!(
        try_call(&mut e, Api::StrCmpNIW, &[0xfffffff0, 0xffffffe0, 0]),
        Err(HostError::Memory(_))
    ));
    let mut e = engine();
    e.map(0x5000, 1, 3).unwrap();
    e.map(0x7000, 1, 3).unwrap();
    e.memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x5ffe), &[b'A', 0])
        .unwrap();
    e.memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x7ffe), &[b'a', 0])
        .unwrap();
    assert!(matches!(
        try_call(&mut e, Api::StrCmpNIW, &[0x5ffe, 0x7ffe, 1]),
        Err(HostError::Memory(_))
    ));
    let mut e = engine();
    wide(&mut e, 0x1200, "ab");
    wide(&mut e, 0x1300, "ac");
    assert_eq!(call(&mut e, Api::StrCmpNIW, &[0x1200, 0x1300, 1]), 0);
    assert_eq!(
        call(&mut e, Api::StrCmpNIW, &[0x1200, 0x1300, u32::MAX]),
        u32::MAX
    );
}
#[test]
fn bounded_dbcs_comparison_reads_exactly_the_requested_bytes_at_the_page_end() {
    let mut e = engine();
    e.configure_nls(NlsProfile32 {
        locale: 0x411,
        ansi: CodePage32::from_id(932).unwrap(),
        oem: CodePage32::from_id(932).unwrap(),
        ..Default::default()
    })
    .unwrap();
    e.map(0x5000, 1, 3).unwrap();
    e.map(0x7000, 1, 3).unwrap();
    e.memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x5ffe), &[0x83, 0x5c])
        .unwrap();
    e.memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x7ffe), &[0x83, 0x5c])
        .unwrap();
    assert_eq!(call(&mut e, Api::StrCmpNIA, &[0x5ffe, 0x7ffe, 2]), 0);
}
#[test]
fn shell_string_comparison_uses_linguistic_instead_of_ordinal_sorting() {
    let mut e = engine();
    wide(&mut e, 0x1200, "b");
    wide(&mut e, 0x1300, "ä");
    assert_eq!(call(&mut e, Api::StrCmpIW, &[0x1200, 0x1300]), 1);
    e.memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1200), &[b'b', 0])
        .unwrap();
    e.memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1300), &[0xe4, 0])
        .unwrap();
    assert_eq!(call(&mut e, Api::StrCmpIA, &[0x1200, 0x1300]), 1);
}

#[test]
fn shell_comparison_keeps_user_collation_when_the_guest_changes_thread_locale() {
    let mut e = engine();
    wide(&mut e, 0x1200, "z");
    wide(&mut e, 0x1300, "ä");
    assert_eq!(
        nls_call(
            &mut e,
            crate::windows::nls::NlsApi32::SetThreadLocale,
            &[0x41d]
        ),
        1
    );
    assert_eq!(call(&mut e, Api::StrCmpIW, &[0x1200, 0x1300]), 1);
    assert_eq!(call(&mut e, Api::StrCmpNIW, &[0x1200, 0x1300, 1]), 1);
    let mut e = engine();
    e.configure_nls(NlsProfile32 {
        locale: 0x41d,
        ..Default::default()
    })
    .unwrap();
    wide(&mut e, 0x1200, "z");
    wide(&mut e, 0x1300, "ä");
    assert_eq!(call(&mut e, Api::StrCmpIW, &[0x1200, 0x1300]), u32::MAX);
}

#[test]
fn native_x86_append_root_and_empty_mutations_do_not_share_combine_semantics() {
    let mut e = engine();
    wide(&mut e, 0x1200, r"C:\one");
    wide(&mut e, 0x1300, r"\two");
    assert_eq!(call(&mut e, Api::PathAppendW, &[0x1200, 0x1300]), 1);
    let output = super::super::strings::Input32::read(
        e.memory.as_ref().unwrap(),
        0x1200,
        true,
        e.windows_ansi_code_page(),
        260,
    )
    .unwrap();
    assert_eq!(
        output.units,
        r"C:\one\two".encode_utf16().collect::<Vec<_>>()
    );
    wide(&mut e, 0x1200, "C:");
    assert_eq!(call(&mut e, Api::PathIsRootW, &[0x1200]), 0);
    wide(&mut e, 0x1200, "");
    assert_eq!(call(&mut e, Api::PathAddBackslashW, &[0x1200]), 0x1200);
    let mut empty = [0xff; 4];
    e.memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(0x1200), &mut empty[..2])
        .unwrap();
    assert_eq!(&empty[..2], &[0; 2]);
    wide(&mut e, 0x1200, r"relative\leaf");
    assert_eq!(call(&mut e, Api::PathStripToRootW, &[0x1200]), 0);
    e.memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(0x1200), &mut empty[..2])
        .unwrap();
    assert_eq!(&empty[..2], &[0; 2]);
}
