use super::document::{ProfileDocument32, integer};
#[test]
fn case_folded_queries_preserve_quoted_values_and_section_order() {
    let document =
        ProfileDocument32::new(";keep\n[Video]\nWidth = 0x400\nTitle=\" max \"\n[Other]\nx=1\n");
    assert_eq!(document.value("vIdEo", "WIDTH").as_deref(), Some("0x400"));
    assert_eq!(document.value("VIDEO", "title").as_deref(), Some(" max "));
    assert_eq!(document.names(None), ["Video", "Other"]);
    assert_eq!(document.names(Some("video")), ["Width", "Title"]);
    assert_eq!(integer(" -42tail"), (-42i32) as u32);
    assert_eq!(integer("0x400"), 1024);
}
#[test]
fn updates_and_deletions_preserve_unrelated_lines() {
    let mut document =
        ProfileDocument32::new(";keep\n[Video]\nWidth=640\nHeight=480\n[Other]\nx=1\n");
    document.set("video", Some("WIDTH"), Some("800"));
    document.set("video", Some("height"), None);
    assert_eq!(
        document.text(),
        ";keep\r\n[Video]\r\nWIDTH=800\r\n[Other]\r\nx=1\r\n"
    );
    document.set("VIDEO", None, None);
    assert_eq!(document.text(), ";keep\r\n[Other]\r\nx=1\r\n");
}
#[test]
fn profile_api_uses_vfs_overlay_default_values_and_double_nul_truncation() {
    use crate::{
        memory::GuestAddress,
        process::windows::files::{ordinary::FileCallOutcome32, tests::support},
        windows::{files::FileApi32 as Api, security::Token32},
    };
    let (mut runtime, mut memory, _) = support::runtime();
    for (at, text) in [
        (0x1000, "Video"),
        (0x1100, "Width"),
        (0x1200, "800"),
        (0x1300, r"C:\settings.ini"),
    ] {
        memory
            .write(GuestAddress(at), &[text.as_bytes(), &[0]].concat())
            .unwrap();
    }
    let token = Token32::virtual_process();
    assert_eq!(
        runtime
            .profile_call(
                &mut memory,
                &token,
                Api::GetPrivateProfileIntA,
                &[0x1000, 0x1100, 640, 0x1300]
            )
            .unwrap(),
        FileCallOutcome32::success(640)
    );
    assert_eq!(
        runtime
            .profile_call(
                &mut memory,
                &token,
                Api::WritePrivateProfileStringA,
                &[0x1000, 0x1100, 0x1200, 0x1300]
            )
            .unwrap(),
        FileCallOutcome32::success(1)
    );
    assert_eq!(
        runtime
            .profile_call(
                &mut memory,
                &token,
                Api::GetPrivateProfileIntA,
                &[0x1000, 0x1100, 640, 0x1300]
            )
            .unwrap(),
        FileCallOutcome32::success(800)
    );
    assert_eq!(
        runtime
            .profile_call(
                &mut memory,
                &token,
                Api::GetPrivateProfileStringA,
                &[0x1000, 0, 0, 0x1800, 4, 0x1300]
            )
            .unwrap(),
        FileCallOutcome32::success(2)
    );
    assert_eq!(support::read::<4>(&memory, 0x1800), *b"Wi\0\0");
    assert_eq!(
        runtime
            .profile_call(
                &mut memory,
                &token,
                Api::GetPrivateProfileStringA,
                &[0x1000, 0x1100, 0, 0x1800, 3, 0x1300]
            )
            .unwrap(),
        FileCallOutcome32::success(2)
    );
    assert_eq!(support::read::<3>(&memory, 0x1800), *b"80\0");
}
