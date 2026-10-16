use super::{returned, support::*};
use crate::{
    memory::GuestAddress,
    windows::{files::FileApi32 as Api, text::CodePage32},
};

#[test]
fn japanese_file_paths_decode_complete_dbcs_and_report_ansi_file_part_in_bytes() {
    let (mut runtime, mut memory, _) = runtime();
    runtime.ordinary.ansi = CodePage32::from_id(932).unwrap();
    let path = [b'C', b':', b'\\', 0x95, 0x5c, b'.', b't', b'x', b't', 0];
    memory.write(GuestAddress(0x1200), &path).unwrap();
    let handle = returned(
        runtime
            .execute_ordinary(
                &mut memory,
                Api::CreateFileA,
                &[0x1200, 0xc0000000, 7, 0, 4, 0, 0],
            )
            .unwrap(),
    )
    .0;
    assert_ne!(handle, u32::MAX);
    assert_eq!(
        runtime
            .vfs
            .file_path(
                runtime
                    .handles
                    .file(handle)
                    .and_then(|open| runtime.vfs.file_for_open(open))
                    .unwrap()
            )
            .unwrap(),
        "C:\\表.txt".encode_utf16().collect::<Vec<_>>()
    );
    assert_eq!(
        returned(
            runtime
                .execute_ordinary(
                    &mut memory,
                    Api::GetFullPathNameA,
                    &[0x1200, 32, 0x1800, 0x1000]
                )
                .unwrap()
        )
        .0,
        9
    );
    assert_eq!(dword(&memory, 0x1000), 0x1803);
    assert_eq!(read::<10>(&memory, 0x1800), path);
    memory.write(GuestAddress(0x1200), b"C:\\*\0").unwrap();
    let find = returned(
        runtime
            .execute_ordinary(&mut memory, Api::FindFirstFileA, &[0x1200, 0x1800])
            .unwrap(),
    )
    .0;
    assert_ne!(find, u32::MAX);
    assert_eq!(
        read::<7>(&memory, 0x1800 + 44),
        [0x95, 0x5c, b'.', b't', b'x', b't', 0]
    );
}
