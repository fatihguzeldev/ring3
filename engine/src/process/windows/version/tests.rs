use crate::{
    abi::{
        arena::{EXIT_OFFSET, TRANSFER_OFFSET},
        x86::{EXIT_SIZE, STATE_SIZE, decode_state, encode_exit_v3, encode_state_current},
    },
    cpu::{ExecutionExit, ExitReason, x86::State32},
    files::{ATTR_NORMAL, AssetRef32, FileTimes32},
    memory::GuestAddress,
    process::{CallError, EngineInstance, HostError, windows::files::FileConfig32},
    windows::{nls::NlsProfile32, text::CodePage32, version::VersionApi32 as Api},
};
const KEY: u64 = 0x5e41;
fn engine() -> EngineInstance {
    let mut e = EngineInstance::new(128, KEY).unwrap();
    e.configure_nls(NlsProfile32 {
        locale: 0x411,
        ansi: CodePage32::from_id(932).unwrap(),
        oem: CodePage32::from_id(932).unwrap(),
        ..Default::default()
    })
    .unwrap();
    e.configure_files(FileConfig32::default()).unwrap();
    e.map(0x1000, 4, 3).unwrap();
    e.map(0x6000, 1, 7).unwrap();
    e.memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x6000), &[0x0f, 0x0b])
        .unwrap();
    e
}
fn capture(e: &mut EngineInstance, api: Api, args: &[u32]) -> u32 {
    for (i, x) in [0x6000u32, 2, 0x6000, api.id()].into_iter().enumerate() {
        e.arena_mut().unwrap()[TRANSFER_OFFSET + i * 4..TRANSFER_OFFSET + i * 4 + 4]
            .copy_from_slice(&x.to_le_bytes());
    }
    e.compile_with_gates(1, 1).unwrap();
    for (i, x) in std::iter::once(0x6002)
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
        eip: 0x6000,
        ..Default::default()
    };
    state.registers[4] = 0x1000;
    encode_state_current(&state, &mut e.arena_mut().unwrap()[..STATE_SIZE]).unwrap();
    encode_exit_v3(
        &ExecutionExit {
            retired: 0,
            reason: ExitReason::Gate { id: api.id() },
        },
        &mut e.arena_mut().unwrap()[EXIT_OFFSET..EXIT_OFFSET + EXIT_SIZE],
    )
    .unwrap();
    let generation = e.generation;
    let token = e.capture_windows_call(KEY, generation).unwrap().token;
    e.complete_windows_call(KEY, generation, token).unwrap();
    token
}
fn result(e: &EngineInstance) -> u32 {
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
fn image() -> Vec<u8> {
    let mut image = super::super::resources::tests::image(0x400000, true, false);
    let block = crate::windows::version::version_fixture();
    image[0x40e..0x410].copy_from_slice(&1u16.to_le_bytes());
    image[0x410..0x414].copy_from_slice(&16u32.to_le_bytes());
    image[0x4b4..0x4b8].copy_from_slice(&(block.len() as u32).to_le_bytes());
    image[0x500..0x500 + block.len()].copy_from_slice(&block);
    image
}
fn finish_bytes(e: &mut EngineInstance, bytes: &[u8]) -> usize {
    let mut waits = 0;
    while let Some(wait) = e.pending_version_call().unwrap() {
        assert!(!wait.ready);
        assert_eq!(
            e.resume_version_call(KEY, wait.token),
            Err(HostError::Call(CallError::Busy))
        );
        let start = wait.request.offset as usize;
        for (index, chunk) in bytes[start..start + wait.request.length as usize]
            .chunks(4096)
            .enumerate()
        {
            e.supply_file_bytes(wait.request, index as u32 * 4096, chunk)
                .unwrap();
        }
        assert!(e.pending_version_call().unwrap().unwrap().ready);
        e.resume_version_call(KEY, wait.token).unwrap();
        waits += 1;
    }
    waits
}
#[test]
fn captured_version_reads_lazy_pe_without_loading_it_and_queries_dbcs_in_the_same_block() {
    let mut e = engine();
    let path = r"C:\version.dll".encode_utf16().collect::<Vec<_>>();
    let bytes = image();
    e.files
        .as_mut()
        .unwrap()
        .vfs_for_test()
        .mount_asset(
            &path,
            AssetRef32 {
                id: 91,
                length: bytes.len() as u64,
            },
            ATTR_NORMAL,
            FileTimes32::default(),
        )
        .unwrap();
    wide(&mut e, 0x1200, r"C:\version.dll");
    let modules = e.loaded_windows_modules().count();
    capture(&mut e, Api::GetFileVersionInfoSizeW, &[0x1200, 0x1300]);
    let pending = e.pending_version_call().unwrap().unwrap();
    assert_eq!(pending.request.asset, 91);
    assert!(finish_bytes(&mut e, &bytes) > 0);
    let size = result(&e);
    assert!(size > crate::windows::version::version_fixture().len() as u32);
    assert_eq!(word(&e, 0x1300), 0);
    assert_eq!(e.loaded_windows_modules().count(), modules);
    capture(&mut e, Api::GetFileVersionInfoW, &[0x1200, 0, size, 0x2000]);
    finish_bytes(&mut e, &bytes);
    assert_eq!(result(&e), 1);
    wide(&mut e, 0x1400, r"\StringFileInfo\041104b0\ProductName");
    capture(
        &mut e,
        Api::VerQueryValueW,
        &[0x2000, 0x1400, 0x1500, 0x1504],
    );
    assert_eq!(result(&e), 1);
    assert_eq!(word(&e, 0x1504), 3);
    let wide_at = word(&e, 0x1500);
    let mut wide_bytes = [0; 6];
    e.memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(wide_at), &mut wide_bytes)
        .unwrap();
    assert_eq!(wide_bytes, [0xfd, 0x88, 0xc1, 0x54, 0, 0]);
    e.memory
        .as_mut()
        .unwrap()
        .write(
            GuestAddress(0x1400),
            b"\\StringFileInfo\\041104b0\\ProductName\0",
        )
        .unwrap();
    capture(
        &mut e,
        Api::VerQueryValueA,
        &[0x2000, 0x1400, 0x1500, 0x1504],
    );
    assert_eq!(result(&e), 1);
    assert_eq!(word(&e, 0x1504), 5);
    let at = word(&e, 0x1500);
    assert!((0x2000..0x2000 + size).contains(&at));
    let mut ansi = [0; 5];
    e.memory
        .as_ref()
        .unwrap()
        .read(GuestAddress(at), &mut ansi)
        .unwrap();
    assert_eq!(ansi, [0x90, 0xbb, 0x95, 0x69, 0]);
}
#[test]
fn query_pointer_and_length_are_atomic_when_the_second_guest_output_faults() {
    let mut e = engine();
    let bytes = crate::windows::version::version_fixture();
    e.memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x2000), &bytes)
        .unwrap();
    wide(&mut e, 0x1400, r"\");
    e.memory
        .as_mut()
        .unwrap()
        .write(GuestAddress(0x1500), &0xfeedbeefu32.to_le_bytes())
        .unwrap();
    let version = e.memory.as_ref().unwrap().transaction_version();
    assert!(matches!(
        e.version_query(Api::VerQueryValueW, &[0x2000, 0x1400, 0x1500, 0xfffffff0]),
        Err(crate::windows::version::VersionError32::Memory(_))
    ));
    assert_eq!(word(&e, 0x1500), 0xfeedbeef);
    assert_eq!(e.memory.as_ref().unwrap().transaction_version(), version);
}
