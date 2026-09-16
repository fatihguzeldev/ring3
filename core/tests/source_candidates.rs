use ring3_core::{
    AsciiApplicationSourceCandidateError as Error, AsciiApplicationSourceCandidateLimits as Limits,
    AsciiSourcePathCollision as Collision, AsciiSourcePathEntry, AsciiSourcePathError as PathError,
    AsciiSourcePathLimits, AsciiSourcePathSegmentError as Segment,
    find_ascii_application_source_candidate as find,
};

fn limits() -> Limits {
    Limits {
        paths: AsciiSourcePathLimits {
            max_paths: 8,
            max_path_bytes: 64,
            max_total_path_bytes: 256,
            max_depth: 4,
        },
        max_basename_bytes: 64,
    }
}

#[test]
fn explicit_context_preserves_owned_entry_and_original_position() {
    let result = {
        let mut paths = ["app.exe", "a.dll", r"Bin\App.exe", r"bin\A.DLL"].map(String::from);
        let mut token = String::from("a.dLl");
        let before = paths.clone();
        let result = find(&paths.each_ref().map(String::as_str), 2, &token, limits());
        assert_eq!(paths, before);
        assert_eq!(token, "a.dLl");
        for path in &mut paths {
            let len = path.len();
            path.clear();
            path.extend(std::iter::repeat_n('x', len));
        }
        token.replace_range(.., "xxxxx");
        result
    };
    assert_eq!(
        result,
        Ok(Some(AsciiSourcePathEntry {
            index: 3,
            normalized: "bin/A.DLL".into(),
            key: "bin/a.dll".into(),
            depth: 2,
        }))
    );
    let reordered = [r"bin\A.DLL", r"Bin\App.exe", "a.dll", "app.exe"];
    let first = find(&reordered, 1, "A.dll", limits()).unwrap().unwrap();
    assert_eq!(first.index, 0);
    assert_eq!(first.normalized, "bin/A.DLL");
    assert_eq!(find(&reordered, 1, "A.dll", limits()), Ok(Some(first)));
}

#[test]
fn only_the_selected_complete_parent_can_match() {
    let paths = ["app.exe", "bin/app.exe", "bin2/a.dll", "bin/sub/a.dll"];
    for application in [0, 1] {
        assert_eq!(find(&paths, application, "a.dll", limits()), Ok(None));
    }
    let arbitrary_context = ["one/context.txt", "two/a.dll", "one/a.dll"];
    assert_eq!(
        find(&arbitrary_context, 0, "A.DLL", limits())
            .unwrap()
            .unwrap()
            .index,
        2
    );
    assert_eq!(
        find(&["app.exe"], 0, "app.exe", limits())
            .unwrap()
            .unwrap()
            .index,
        0
    );
}

#[test]
fn basenames_stay_literal_without_extension_or_percent_transforms() {
    for basename in [
        "thing",
        "%2f.dll",
        ".hidden",
        " leading.dll",
        "a b.dll",
        "com10.dll",
    ] {
        let paths = ["app.exe", basename];
        let candidate = find(&paths, 0, basename, limits()).unwrap().unwrap();
        assert_eq!(
            (candidate.index, candidate.normalized.as_str()),
            (1, basename)
        );
    }
    assert_eq!(
        find(&["app.exe", "thing.dll"], 0, "thing", limits()),
        Ok(None)
    );
    assert_eq!(
        find(&["app.exe", "thing"], 0, "thing.dll", limits()),
        Ok(None)
    );
}

#[test]
fn whole_path_admission_precedes_index_then_basename() {
    assert_eq!(
        find(&["app.exe", "a.dll", "bad/../file"], 99, "", limits()),
        Err(Error::Paths(PathError::Segment {
            index: 2,
            segment: 1,
            reason: Segment::Parent
        }))
    );
    assert_eq!(
        find(&["app.exe"], 1, "", limits()),
        Err(Error::ApplicationSourceIndexOutOfRange { index: 1, count: 1 })
    );
    assert_eq!(
        find(&[], 0, "a.dll", limits()),
        Err(Error::ApplicationSourceIndexOutOfRange { index: 0, count: 0 })
    );
    assert_eq!(
        find(&["app.exe"], 0, "", limits()),
        Err(Error::Basename(PathError::EmptyPath { index: 0 }))
    );
    let mut bounded = limits();
    bounded.paths.max_path_bytes = 8;
    assert_eq!(
        find(&["..", "123456789"], 99, "", bounded),
        Err(Error::Paths(PathError::PathBytesExceeded {
            index: 1,
            bytes: 9,
            limit: 8
        }))
    );
}

#[test]
fn basename_admission_preserves_error_operands_and_priority() {
    let cases = [
        ("/é", PathError::AbsolutePath { index: 0 }),
        (r"\a", PathError::AbsolutePath { index: 0 }),
        (
            "a/b",
            PathError::DepthExceeded {
                index: 0,
                depth: 2,
                limit: 1,
            },
        ),
        (
            r"a\b",
            PathError::DepthExceeded {
                index: 0,
                depth: 2,
                limit: 1,
            },
        ),
        (
            "a/é",
            PathError::NonAsciiByte {
                index: 0,
                offset: 2,
            },
        ),
        (
            "a/:b",
            PathError::ForbiddenByte {
                index: 0,
                offset: 2,
                byte: b':',
            },
        ),
        (
            "a\0b",
            PathError::ForbiddenByte {
                index: 0,
                offset: 1,
                byte: 0,
            },
        ),
        (
            "..",
            PathError::Segment {
                index: 0,
                segment: 0,
                reason: Segment::Parent,
            },
        ),
        (
            "a.",
            PathError::Segment {
                index: 0,
                segment: 0,
                reason: Segment::TrailingDotOrSpace,
            },
        ),
        (
            "CON .dll",
            PathError::Segment {
                index: 0,
                segment: 0,
                reason: Segment::ReservedDeviceStem,
            },
        ),
    ];
    for (basename, error) in cases {
        assert_eq!(
            find(&["app.exe"], 0, basename, limits()),
            Err(Error::Basename(error)),
            "{basename:?}"
        );
    }
}

#[test]
fn exact_limits_succeed_and_zero_limits_remain_explicit() {
    let exact = Limits {
        paths: AsciiSourcePathLimits {
            max_paths: 2,
            max_path_bytes: 7,
            max_total_path_bytes: 12,
            max_depth: 1,
        },
        max_basename_bytes: 5,
    };
    let paths = ["app.exe", "a.dll"];
    assert!(find(&paths, 0, "a.dll", exact).unwrap().is_some());
    let mut bounded = exact;
    bounded.max_basename_bytes = 4;
    assert_eq!(
        find(&paths, 0, "a.dll", bounded),
        Err(Error::Basename(PathError::PathBytesExceeded {
            index: 0,
            bytes: 5,
            limit: 4
        }))
    );
    bounded.max_basename_bytes = 0;
    assert_eq!(
        find(&paths, 0, "a", bounded),
        Err(Error::Basename(PathError::PathBytesExceeded {
            index: 0,
            bytes: 1,
            limit: 0
        }))
    );
    assert_eq!(
        find(&paths, 0, "", bounded),
        Err(Error::Basename(PathError::EmptyPath { index: 0 }))
    );
    bounded = exact;
    bounded.paths.max_total_path_bytes = 11;
    assert_eq!(
        find(&paths, 0, "a.dll", bounded),
        Err(Error::Paths(PathError::TotalPathBytesExceeded {
            index: 1,
            total: 12,
            limit: 11
        }))
    );
    bounded.paths.max_paths = 0;
    assert_eq!(
        find(&paths, 0, "a.dll", bounded),
        Err(Error::Paths(PathError::PathCountExceeded {
            count: 2,
            limit: 0
        }))
    );
    let unbounded = Limits {
        paths: AsciiSourcePathLimits {
            max_paths: u64::MAX,
            max_path_bytes: u64::MAX,
            max_total_path_bytes: u64::MAX,
            max_depth: u64::MAX,
        },
        max_basename_bytes: u64::MAX,
    };
    assert_eq!(
        find(&paths, 0, "a.dll", unbounded),
        find(&paths, 0, "a.dll", exact)
    );
}

#[test]
fn later_collisions_reject_an_earlier_apparent_match() {
    for (paths, error) in [
        (
            vec!["app.exe", "a.dll", "A.DLL"],
            PathError::Collision {
                index: 2,
                prior: 1,
                kind: Collision::Duplicate,
            },
        ),
        (
            vec!["app.exe", "a.dll", "a.dll/child"],
            PathError::Collision {
                index: 2,
                prior: 1,
                kind: Collision::AncestorFile,
            },
        ),
        (
            vec!["app.exe", "dir/z", "dir/a", "dir"],
            PathError::Collision {
                index: 3,
                prior: 2,
                kind: Collision::DescendantFile,
            },
        ),
    ] {
        assert_eq!(find(&paths, 0, "a.dll", limits()), Err(Error::Paths(error)));
    }
}
