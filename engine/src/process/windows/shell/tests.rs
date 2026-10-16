use crate::{
    abi::{
        arena::{EXIT_OFFSET, TRANSFER_OFFSET},
        x86::{EXIT_SIZE, STATE_SIZE, decode_state, encode_exit_v3, encode_state_current},
    },
    cpu::{ExecutionExit, ExitReason, x86::State32},
    memory::GuestAddress,
    process::{EngineInstance, HostError, windows::files::FileConfig32},
    windows::{memory::MemoryApi32, shell::ShellApi32 as Api},
};
const KEY: u64 = 0x5e11;
fn engine() -> EngineInstance {
    let mut e = EngineInstance::new(128, KEY).unwrap();
    e.map(0x1000, 2, 3).unwrap();
    e.map(0x3000, 1, 7).unwrap();
    e.memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x3000), &[0x0f, 0x0b])
        .unwrap();
    e
}
fn call(e: &mut EngineInstance, id: u32, args: &[u32]) -> u32 {
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
            reason: ExitReason::Gate { id },
        },
        &mut e.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    let generation = e.generation;
    let token = e.capture_windows_call(KEY, generation).unwrap().token;
    e.complete_windows_call(KEY, generation, token).unwrap();
    decode_state(&e.arena()[..STATE_SIZE]).unwrap().registers[0]
}
fn word(e: &EngineInstance, at: u32) -> u32 {
    let mut bytes = [0; 4];
    e.memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(at), &mut bytes)
        .unwrap();
    u32::from_le_bytes(bytes)
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
fn text(e: &EngineInstance, at: u32) -> Vec<u16> {
    let mut out = Vec::new();
    for index in 0..32768 {
        let mut b = [0; 2];
        e.memory
            .as_ref()
            .unwrap()
            .read(GuestAddress(at + index * 2), &mut b)
            .unwrap();
        let unit = u16::from_le_bytes(b);
        if unit == 0 {
            return out;
        }
        out.push(unit);
    }
    panic!("unterminated test output")
}
#[test]
fn captured_shell_arguments_are_one_local_free_allocation_with_exact_interior_pointers() {
    let mut e = engine();
    wide(&mut e, 0x1200, r#"app.exe "two words" c\"d"#);
    let base = call(&mut e, Api::CommandLineToArgvW.id(), &[0x1200, 0x1300]);
    assert_ne!(base, 0);
    assert_eq!(word(&e, 0x1300), 3);
    let expected = ["app.exe", "two words", "c\"d"];
    let mut next = base + 12;
    for (index, expected) in expected.into_iter().enumerate() {
        let pointer = word(&e, base + index as u32 * 4);
        assert_eq!(pointer, next);
        let units = expected.encode_utf16().collect::<Vec<_>>();
        assert_eq!(text(&e, pointer), units);
        next += (units.len() as u32 + 1) * 2;
    }
    assert_eq!(call(&mut e, MemoryApi32::LocalFree.id(), &[base]), 0);
}
#[test]
fn invalid_argument_count_pointer_precedes_lazy_heap_creation_and_payload_publication() {
    let mut e = engine();
    wide(&mut e, 0x1200, "app a");
    let pages = e.memory.as_ref().unwrap().mapped_pages();
    let version = e.memory.as_ref().unwrap().transaction_version();
    assert!(e.process_heap().is_none());
    assert!(matches!(
        e.shell_arguments(&[0x1200, 0xfffffff0]),
        Err(HostError::Memory(_))
    ));
    assert!(e.process_heap().is_none());
    assert_eq!(e.memory.as_ref().unwrap().mapped_pages(), pages);
    assert_eq!(e.memory.as_ref().unwrap().transaction_version(), version);
}
#[test]
fn captured_folder_queries_distinguish_verify_create_and_dont_verify_in_virtual_vfs() {
    let mut e = engine();
    e.configure_files(FileConfig32::default()).unwrap();
    let path = r"C:\Users\Player\AppData\Local"
        .encode_utf16()
        .collect::<Vec<_>>();
    assert!(e.provider_path_info(&path).is_err());
    assert_eq!(
        call(
            &mut e,
            Api::SHGetFolderPathW.id(),
            &[0, 0x401c, 0, 0, 0x1400]
        ),
        0
    );
    assert_eq!(text(&e, 0x1400), path);
    assert!(e.provider_path_info(&path).is_err());
    assert_eq!(
        call(&mut e, Api::SHGetFolderPathW.id(), &[0, 0x1c, 0, 0, 0x1400]),
        0x80070002
    );
    assert!(text(&e, 0x1400).is_empty());
    assert_eq!(
        call(
            &mut e,
            Api::SHGetFolderPathW.id(),
            &[0, 0x801c, 0, 0, 0x1400]
        ),
        0
    );
    assert_eq!(text(&e, 0x1400), path);
    assert_eq!(e.provider_path_info(&path).unwrap().attributes & 0x10, 0x10);
}
#[test]
fn folder_create_output_fault_leaves_every_missing_ancestor_unpublished() {
    let mut e = engine();
    e.configure_files(FileConfig32::default()).unwrap();
    let path = r"C:\Users\Player\AppData\Local"
        .encode_utf16()
        .collect::<Vec<_>>();
    assert!(matches!(
        e.shell_folder(Api::SHGetFolderPathW, &[0, 0x801c, 0, 0, 0xfffffff0]),
        Err(HostError::Memory(_))
    ));
    assert!(e.provider_path_info(&path).is_err());
    assert!(
        e.provider_path_info(&r"C:\Users".encode_utf16().collect::<Vec<_>>())
            .is_err()
    );
}

#[test]
fn recursive_folder_capacity_failure_does_not_publish_a_prefix() {
    let mut e = engine();
    e.configure_files(FileConfig32 {
        vfs: crate::files::FileLimits32 {
            nodes: 6,
            ..Default::default()
        },
        ..Default::default()
    })
    .unwrap();
    let revision = e.files.as_mut().unwrap().vfs_for_test().revision();
    assert_eq!(
        call(
            &mut e,
            Api::SHGetFolderPathW.id(),
            &[0, 0x801c, 0, 0, 0x1400]
        ),
        0x80070008
    );
    assert_eq!(
        e.files.as_mut().unwrap().vfs_for_test().revision(),
        revision
    );
    assert!(text(&e, 0x1400).is_empty());
    assert!(
        e.provider_path_info(&r"C:\Users".encode_utf16().collect::<Vec<_>>())
            .is_err()
    );
}
