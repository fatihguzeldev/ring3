use super::*;
fn w(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

#[test]
fn documented_canonical_paths_preserve_root_and_remove_navigation() {
    for (input, expected) in [
        (r"A:\name_1\.\name_2\..\name_3", r"A:\name_1\name_3"),
        (r"C:\..", r"C:\"),
        (r"\\server\share\dir\..\..\..", r"\\server\share"),
    ] {
        assert_eq!(canonicalize(&w(input)).unwrap(), w(expected));
    }
}
#[test]
fn rooted_combine_and_pointer_offsets_are_lexical_not_vfs_queries() {
    assert_eq!(
        combine(Some(&w("C:")), Some(&w(r"One\Two\Three"))).unwrap(),
        w(r"C:\One\Two\Three")
    );
    assert_eq!(
        combine(Some(&w(r"C:\one\two")), Some(&w(r"\three"))).unwrap(),
        w(r"C:\three")
    );
    assert!(!is_relative(&w("C:relative")));
    assert_eq!(file_name(&w(r"C:\dir.name\file.ext")), 12);
    assert_eq!(extension(&w(r"C:\dir.name\file.ext")), 16);
    assert_eq!(remove_file_spec(&w(r"C:\file")), 3);
}
#[test]
fn catalog_is_closed_and_declares_literal_legacy_stack_shapes() {
    assert_eq!(ShlwapiApi32::PathCombineW.id(), 0x001c_0002);
    assert_eq!(ShlwapiApi32::PathCombineW.stack_words(), 3);
    assert_eq!(ShlwapiApi32::StrStrIW.stack_words(), 2);
    assert_eq!(ShlwapiApi32::from_id(0x001c_ffff), None);
    assert_eq!(
        shlwapi_module(0x7100_0000).exports.len(),
        ShlwapiApi32::ALL.len()
    );
}

#[test]
fn native_x86_canonicalization_preserves_drive_relative_slashes_and_parent_boundaries() {
    // Windows run 38958832841, authored oracle fc66cd9e.
    for (input, expected) in [
        ("", "\\"),
        (".", "\\"),
        ("..", "\\"),
        (".\\a", "a"),
        ("..\\a", "a"),
        ("a\\..\\b", "\\b"),
        ("C:", "C:\\"),
        ("C:foo", "C:foo"),
        ("C:\\", "C:\\"),
        ("C:\\foo\\", "C:\\foo\\"),
        ("C:\\foo.bar space", "C:\\foo.bar space"),
        ("\\\\server", "\\\\server"),
        ("\\\\server\\share", "\\\\server\\share"),
        ("\\\\server\\share\\", "\\\\server\\share\\"),
        ("a/b.txt", "a/b.txt"),
        ("a..b", "a..b"),
        ("C:\\folder\\..\\target", "C:\\target"),
        ("C:\\..\\target", "C:\\target"),
    ] {
        assert_eq!(canonicalize(&w(input)).unwrap(), w(expected), "{input}");
    }
}

#[test]
fn native_x86_pointer_helpers_keep_trailing_components_and_reset_extensions_after_spaces() {
    for (input, name, extension_at, remove) in [
        ("", 0, 0, 0),
        (".", 0, 0, 0),
        ("..", 0, 1, 0),
        (".\\a", 2, 3, 1),
        ("..\\a", 3, 4, 2),
        ("a\\..\\b", 5, 6, 4),
        ("C:", 0, 2, 2),
        ("C:foo", 2, 5, 2),
        ("C:\\", 0, 3, 3),
        ("C:\\foo\\", 3, 7, 6),
        ("C:\\foo.bar space", 3, 16, 3),
        ("\\\\server", 2, 8, 2),
        ("\\\\server\\share", 9, 14, 8),
        ("\\\\server\\share\\", 9, 15, 14),
        ("a/b.txt", 2, 3, 0),
        ("a..b", 0, 2, 0),
        ("C:\\folder\\..\\target", 13, 19, 12),
        ("C:\\..\\target", 6, 12, 5),
    ] {
        let path = w(input);
        assert_eq!(file_name(&path), name, "file name {input}");
        assert_eq!(extension(&path), extension_at, "extension {input}");
        assert_eq!(remove_file_spec(&path), remove, "remove {input}");
    }
}
