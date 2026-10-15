use super::*;
use crate::files::FileError32;
fn units(value: &str) -> Vec<u16> {
    value.encode_utf16().collect()
}
#[test]
fn virtual_roots_normalize_without_reaching_a_host_path() {
    let cwd = units(r"C:\games\demo");
    assert_eq!(
        normalize(&units(r"..\assets\tone. "), &cwd, false).unwrap(),
        units(r"C:\games\assets\tone")
    );
    assert_eq!(
        normalize(&units(r"\..\..\saves"), &cwd, false).unwrap(),
        units(r"C:\saves")
    );
    assert_eq!(
        normalize(&units(r"C:\NUL.txt"), &cwd, false),
        Err(FileError32::Unsupported)
    );
    assert_eq!(
        normalize(&units(r"\\.\PhysicalDrive0"), &cwd, false),
        Err(FileError32::Unsupported)
    );
    assert_eq!(
        normalize(&units(r"C:\asset:stream"), &cwd, false),
        Err(FileError32::Unsupported)
    );
}
#[test]
fn extended_names_and_unpaired_utf16_keep_distinct_identity() {
    let cwd = units(r"C:\");
    assert_eq!(
        normalize(&units(r"\\?\C:\asset. "), &cwd, false).unwrap(),
        units("C:\\asset. ")
    );
    let path = [67, 58, 92, 0xd800, 46, 98];
    assert_eq!(normalize(&path, &cwd, false).unwrap(), path);
    assert_eq!(fold(&[0xd800]), [0xd800]);
    assert_ne!(fold(&units("ß")), fold(&units("SS")));
}
#[test]
fn dos_wildcards_distinguish_extension_and_optional_question_mark() {
    assert!(matches(&units("*.*"), &units("LICENSE")));
    assert!(matches(&units("*."), &units("LICENSE")));
    assert!(!matches(&units("*."), &units("asset.bin")));
    assert!(matches(&units("a?.txt"), &units("a.txt")));
    assert!(matches(&units("a?.txt"), &units("ab.txt")));
    assert!(!matches(&units("a?.txt"), &units("abc.txt")));
}
