use super::*;
fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}
#[test]
fn command_arguments_preserve_leading_empty_quotes_and_slash_parity() {
    for (source, expected) in [
        ("", vec!["C:\\app.exe"]),
        ("   one two ", vec!["", "one", "two"]),
        (
            r#""C:\long name\app.exe" "a b" c\\\"d"#,
            vec![r"C:\long name\app.exe", "a b", r#"c\"d"#],
        ),
        ("app \"\" tail", vec!["app", "", "tail"]),
    ] {
        assert_eq!(
            command_line_arguments(&w(source), &w(r"C:\app.exe")).unwrap(),
            expected.iter().map(|s| w(s)).collect::<Vec<_>>()
        );
    }
}
#[test]
fn virtual_folder_profile_never_reads_host_environment_and_rejects_virtual_nonpaths() {
    let profile = ShellFolderProfile32::default();
    profile.validate().unwrap();
    assert_eq!(
        profile
            .path(0x1c, false, &w(r"D:\Win"), &w(r"D:\Win\System32"))
            .unwrap(),
        w(r"C:\Users\Player\AppData\Local")
    );
    assert_eq!(
        profile
            .path(0x25, false, &w(r"D:\Win"), &w(r"D:\Win\System32"))
            .unwrap(),
        w(r"D:\Win\System32")
    );
    assert_eq!(
        profile.path(0x11, false, &[], &[]),
        Err(ShellError32::InvalidParameter)
    );
    assert_eq!(ShellApi32::CommandLineToArgvW.stack_words(), 2);
}
