use super::{returned, support::*};
use crate::{
    files::{FileAccess32, FileError32},
    memory::GuestAddress,
    windows::files::FileApi32 as Api,
};

#[test]
fn configured_system_directories_use_win32_argument_order_and_bounded_outputs() {
    let (mut runtime, mut memory, _) = runtime();
    let mut environment = crate::process::windows::files::FileEnvironment32::default();
    environment.windows_directory = "C:\\Profile\\Windows".encode_utf16().collect();
    environment.system_directory = "C:\\Profile\\System32".encode_utf16().collect();
    runtime.set_file_environment(environment).unwrap();
    for (id, name, expected, wide) in [
        (
            0x000a_00af,
            "GetWindowsDirectoryA",
            "C:\\Profile\\Windows",
            false,
        ),
        (
            0x000a_00b0,
            "GetWindowsDirectoryW",
            "C:\\Profile\\Windows",
            true,
        ),
        (
            0x000a_00b1,
            "GetSystemDirectoryA",
            "C:\\Profile\\System32",
            false,
        ),
        (
            0x000a_00b2,
            "GetSystemDirectoryW",
            "C:\\Profile\\System32",
            true,
        ),
    ] {
        let api = Api::from_id(id).expect("system directory service is exported");
        assert_eq!(api.name(), name);
        memory.write(GuestAddress(0x1800), &[0x5a; 64]).unwrap();
        let needed = expected.len() as u32 + 1;
        assert_eq!(
            returned(
                runtime
                    .execute_ordinary(&mut memory, api, &[0x1800, needed - 1])
                    .unwrap()
            )
            .0,
            needed
        );
        assert_eq!(read::<64>(&memory, 0x1800), [0x5a; 64]);
        assert_eq!(
            returned(
                runtime
                    .execute_ordinary(&mut memory, api, &[0x1800, needed])
                    .unwrap()
            )
            .0,
            needed - 1
        );
        let expected_bytes: Vec<u8> = if wide {
            expected
                .encode_utf16()
                .chain([0])
                .flat_map(u16::to_le_bytes)
                .collect()
        } else {
            expected.bytes().chain([0]).collect()
        };
        let mut actual = vec![0; expected_bytes.len()];
        memory.read(GuestAddress(0x1800), &mut actual).unwrap();
        assert_eq!(actual, expected_bytes);
        assert_eq!(
            runtime.execute_ordinary(&mut memory, api, &[0x6000, needed]),
            Err(FileError32::NoAccess)
        );
    }
}

#[test]
fn full_path_required_size_includes_nul_and_file_part_uses_utf16_byte_address() {
    let (mut runtime, mut memory, _) = runtime();
    path(&mut memory, 0x1200, "foo\\..\\bar");
    memory
        .write(GuestAddress(0x1000), &0xdead_beefu32.to_le_bytes())
        .unwrap();
    assert_eq!(
        returned(
            runtime
                .execute_ordinary(&mut memory, Api::GetFullPathNameW, &[0x1200, 0, 0, 0x1000])
                .unwrap()
        )
        .0,
        7
    );
    assert_eq!(dword(&memory, 0x1000), 0xdead_beef);
    assert_eq!(
        returned(
            runtime
                .execute_ordinary(
                    &mut memory,
                    Api::GetFullPathNameW,
                    &[0x1200, 32, 0x1800, 0x1000]
                )
                .unwrap()
        )
        .0,
        6
    );
    assert_eq!(dword(&memory, 0x1000), 0x1806);
    assert_eq!(
        read::<14>(&memory, 0x1800),
        [67, 0, 58, 0, 92, 0, 98, 0, 97, 0, 114, 0, 0, 0]
    );
}

#[test]
fn find_output_fault_preserves_enumeration_and_ansi_wide_calls_share_snapshot() {
    let (mut runtime, mut memory, _) = runtime();
    open(&mut runtime, "C:\\alpha.txt", FileAccess32::ALL);
    open(&mut runtime, "C:\\BETA", FileAccess32::ALL);
    path(&mut memory, 0x1200, "C:\\*.*");
    let find = returned(
        runtime
            .execute_ordinary(&mut memory, Api::FindFirstFileW, &[0x1200, 0x1800])
            .unwrap(),
    )
    .0;
    assert_eq!(
        runtime.execute_ordinary(&mut memory, Api::FindNextFileA, &[find, 0x6000]),
        Err(FileError32::NoAccess)
    );
    runtime
        .execute_ordinary(&mut memory, Api::FindNextFileA, &[find, 0x1800])
        .unwrap();
    assert_eq!(read::<5>(&memory, 0x1800 + 44), *b"BETA\0");
    assert_eq!(
        runtime.execute_ordinary(&mut memory, Api::FindNextFileW, &[find, 0x1800]),
        Err(FileError32::NoMoreFiles)
    );
    runtime
        .execute_ordinary(&mut memory, Api::FindClose, &[find])
        .unwrap();
    assert!(runtime.handles.object(find).is_err());
    assert_eq!(
        runtime.execute_ordinary(&mut memory, Api::FindClose, &[find]),
        Err(FileError32::InvalidHandle)
    );
}
