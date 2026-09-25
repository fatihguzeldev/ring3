use ring3_core::{
    AsciiSourcePathBatch, AsciiSourcePathCollision as Collision, AsciiSourcePathError,
    AsciiSourcePathLimits, AsciiSourcePathSegmentError as Segment, admit_ascii_source_paths,
};

fn limits() -> AsciiSourcePathLimits {
    AsciiSourcePathLimits {
        max_paths: 4,
        max_path_bytes: 64,
        max_total_path_bytes: 128,
        max_depth: 4,
    }
}

#[test]
fn count_refusal_precedes_lexical_errors_and_empty_lists_need_no_budget() {
    let zero = AsciiSourcePathLimits {
        max_paths: 0,
        max_path_bytes: 0,
        max_total_path_bytes: 0,
        max_depth: 0,
    };
    assert_eq!(
        admit_ascii_source_paths(&["../escape"], zero),
        Err(AsciiSourcePathError::PathCountExceeded { count: 1, limit: 0 })
    );
    assert_eq!(
        admit_ascii_source_paths(&[], zero),
        Ok(AsciiSourcePathBatch {
            total_path_bytes: 0,
            entries: vec![],
        })
    );
    assert!(admit_ascii_source_paths(&["game.exe"], limits()).is_ok());
}

#[test]
fn separator_changes_and_ascii_keys_preserve_original_spelling_and_percent_text() {
    let result = admit_ascii_source_paths(&[r"Data\LEVEL.bin", "data/Menu.ini"], limits()).unwrap();
    assert_eq!(result.total_path_bytes, 27);
    for (entry, (normalized, key)) in result.entries.iter().zip([
        ("Data/LEVEL.bin", "data/level.bin"),
        ("data/Menu.ini", "data/menu.ini"),
    ]) {
        assert_eq!(
            (&*entry.normalized, &*entry.key, entry.depth),
            (normalized, key, 2)
        );
    }
    let percent = admit_ascii_source_paths(&["a/%2e%2e", ".hidden"], limits()).unwrap();
    assert_eq!(percent.entries[0].normalized, "a/%2e%2e");
    assert_eq!(percent.entries[0].key, "a/%2e%2e");
    assert_eq!(percent.entries[1].normalized, ".hidden");
}

#[test]
fn all_raw_byte_budgets_precede_earlier_lexical_errors() {
    assert_eq!(
        admit_ascii_source_paths(
            &["..", "later"],
            AsciiSourcePathLimits {
                max_path_bytes: 4,
                ..limits()
            }
        ),
        Err(AsciiSourcePathError::PathBytesExceeded {
            index: 1,
            bytes: 5,
            limit: 4
        })
    );
    assert_eq!(
        admit_ascii_source_paths(
            &["..", "later"],
            AsciiSourcePathLimits {
                max_total_path_bytes: 6,
                ..limits()
            }
        ),
        Err(AsciiSourcePathError::TotalPathBytesExceeded {
            index: 1,
            total: 7,
            limit: 6
        })
    );
    assert_eq!(
        admit_ascii_source_paths(
            &["é"],
            AsciiSourcePathLimits {
                max_path_bytes: 1,
                ..limits()
            }
        ),
        Err(AsciiSourcePathError::PathBytesExceeded {
            index: 0,
            bytes: 2,
            limit: 1
        })
    );
}

#[test]
fn raw_scan_depth_and_segments_preserve_refusal_priority() {
    for (path, expected) in [
        ("", AsciiSourcePathError::EmptyPath { index: 0 }),
        ("/é", AsciiSourcePathError::AbsolutePath { index: 0 }),
        (r"\root", AsciiSourcePathError::AbsolutePath { index: 0 }),
        (
            "a/é?",
            AsciiSourcePathError::NonAsciiByte {
                index: 0,
                offset: 2,
            },
        ),
        (
            "a:?é",
            AsciiSourcePathError::ForbiddenByte {
                index: 0,
                offset: 1,
                byte: b':',
            },
        ),
        (
            "a/./b?",
            AsciiSourcePathError::ForbiddenByte {
                index: 0,
                offset: 5,
                byte: b'?',
            },
        ),
        (
            "a/\0",
            AsciiSourcePathError::ForbiddenByte {
                index: 0,
                offset: 2,
                byte: 0,
            },
        ),
        (
            "a/\u{7f}",
            AsciiSourcePathError::ForbiddenByte {
                index: 0,
                offset: 2,
                byte: 127,
            },
        ),
    ] {
        assert_eq!(admit_ascii_source_paths(&[path], limits()), Err(expected));
    }
    assert_eq!(
        admit_ascii_source_paths(
            &["a//"],
            AsciiSourcePathLimits {
                max_depth: 1,
                ..limits()
            }
        ),
        Err(AsciiSourcePathError::DepthExceeded {
            index: 0,
            depth: 3,
            limit: 1
        })
    );
}

#[test]
fn segment_and_device_stem_rules_are_explicit_and_conservative() {
    for (path, reason) in [
        ("a//b", Segment::Empty),
        ("a/./b", Segment::Dot),
        ("a/../b", Segment::Parent),
        ("a/b.", Segment::TrailingDotOrSpace),
        ("a/CON ", Segment::TrailingDotOrSpace),
        ("a/CON .txt", Segment::ReservedDeviceStem),
        ("a/lPt9.bin", Segment::ReservedDeviceStem),
        ("a/com1", Segment::ReservedDeviceStem),
        ("a/nUl.more.dots", Segment::ReservedDeviceStem),
    ] {
        assert_eq!(
            admit_ascii_source_paths(&[path], limits()),
            Err(AsciiSourcePathError::Segment {
                index: 0,
                segment: 1,
                reason
            })
        );
    }
    for path in ["a/COM0", "a/COM10", "a/LPT0", "a/.aux", "a/conifer"] {
        assert!(admit_ascii_source_paths(&[path], limits()).is_ok());
    }
}

#[test]
fn collisions_report_input_indices_and_choose_the_first_lexical_descendant() {
    for (paths, index, prior, kind) in [
        (vec!["Data/A", r"data\a"], 1, 0, Collision::Duplicate),
        (vec!["a", "a/b"], 1, 0, Collision::AncestorFile),
        (vec!["a/b", "a"], 1, 0, Collision::DescendantFile),
        (vec!["a!", "a/b", "a"], 2, 1, Collision::DescendantFile),
        (vec!["a/z", "a/b", "a"], 2, 1, Collision::DescendantFile),
    ] {
        assert_eq!(
            admit_ascii_source_paths(&paths, limits()),
            Err(AsciiSourcePathError::Collision { index, prior, kind })
        );
    }
    assert!(admit_ascii_source_paths(&["A/x", "a/y", "ab/file"], limits()).is_ok());
    assert!(admit_ascii_source_paths(&["a", "ab/file"], limits()).is_ok());
}

#[test]
fn all_four_limits_accept_exact_bounds_and_reject_one_less() {
    let paths = ["a/b", "C"];
    let exact = AsciiSourcePathLimits {
        max_paths: 2,
        max_path_bytes: 3,
        max_total_path_bytes: 4,
        max_depth: 2,
    };
    let result = admit_ascii_source_paths(&paths, exact).unwrap();
    assert_eq!(result.total_path_bytes, 4);
    assert_eq!(
        result
            .entries
            .iter()
            .map(|entry| entry.index)
            .collect::<Vec<_>>(),
        [0, 1]
    );
    for (cap, error) in [
        (
            AsciiSourcePathLimits {
                max_paths: 1,
                ..exact
            },
            AsciiSourcePathError::PathCountExceeded { count: 2, limit: 1 },
        ),
        (
            AsciiSourcePathLimits {
                max_path_bytes: 2,
                ..exact
            },
            AsciiSourcePathError::PathBytesExceeded {
                index: 0,
                bytes: 3,
                limit: 2,
            },
        ),
        (
            AsciiSourcePathLimits {
                max_total_path_bytes: 3,
                ..exact
            },
            AsciiSourcePathError::TotalPathBytesExceeded {
                index: 1,
                total: 4,
                limit: 3,
            },
        ),
        (
            AsciiSourcePathLimits {
                max_depth: 1,
                ..exact
            },
            AsciiSourcePathError::DepthExceeded {
                index: 0,
                depth: 2,
                limit: 1,
            },
        ),
    ] {
        assert_eq!(admit_ascii_source_paths(&paths, cap), Err(error));
    }
}

#[test]
fn owned_results_outlive_input_strings_and_admission_does_not_modify_them() {
    let result = {
        let inputs = [String::from(r"Data\ONE.bin"), String::from("data/TWO.bin")];
        let before = inputs.clone();
        let refs: Vec<_> = inputs.iter().map(String::as_str).collect();
        let first = admit_ascii_source_paths(&refs, limits()).unwrap();
        assert_eq!(admit_ascii_source_paths(&refs, limits()).unwrap(), first);
        assert_eq!(inputs, before);
        first
    };
    assert_eq!(result.entries[0].normalized, "Data/ONE.bin");
    assert_eq!(result.entries[1].key, "data/two.bin");
}
