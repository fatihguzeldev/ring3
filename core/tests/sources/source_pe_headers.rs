use ring3_core::{
    AsciiPeSource, AsciiPeSourceHeader, AsciiPeSourceHeaderError, AsciiPeSourceHeaderLimits,
    AsciiPeSourceHeaders, AsciiSourcePathCollision, AsciiSourcePathEntry, AsciiSourcePathError,
    AsciiSourcePathLimits, FileOffset, PeHeaderBatchError, PeHeaderBatchLimits, PeHeaderError,
    PeHeaderPrefix, PeKind, parse_ascii_pe_source_headers,
};

#[test]
fn empty_sources_obey_zero_limits() {
    let result = parse_ascii_pe_source_headers(
        &[],
        AsciiPeSourceHeaderLimits {
            paths: AsciiSourcePathLimits {
                max_paths: 0,
                max_path_bytes: 0,
                max_total_path_bytes: 0,
                max_depth: 0,
            },
            headers: PeHeaderBatchLimits {
                max_files: 0,
                max_file_bytes: 0,
                max_total_bytes: 0,
            },
        },
    )
    .unwrap();
    assert_eq!(result.total_path_bytes, 0);
    assert_eq!(result.total_content_bytes, 0);
    assert!(result.entries.is_empty());
}

fn limits() -> AsciiPeSourceHeaderLimits {
    AsciiPeSourceHeaderLimits {
        paths: AsciiSourcePathLimits {
            max_paths: 3,
            max_path_bytes: 10,
            max_total_path_bytes: 23,
            max_depth: 2,
        },
        headers: PeHeaderBatchLimits {
            max_files: 3,
            max_file_bytes: 90,
            max_total_bytes: 180,
        },
    }
}

fn prefix(machine: u16, magic: u16, sections: u16) -> Vec<u8> {
    let mut bytes = vec![0; 90];
    bytes[..2].copy_from_slice(b"MZ");
    bytes[60..64].copy_from_slice(&64_u32.to_le_bytes());
    bytes[64..68].copy_from_slice(b"PE\0\0");
    bytes[68..70].copy_from_slice(&machine.to_le_bytes());
    bytes[70..72].copy_from_slice(&sections.to_le_bytes());
    bytes[84..86].copy_from_slice(&2_u16.to_le_bytes());
    bytes[86..88].copy_from_slice(&0x4321_u16.to_le_bytes());
    bytes[88..90].copy_from_slice(&magic.to_le_bytes());
    bytes
}

fn expected_prefix(machine: u16, kind: PeKind, number_of_sections: u16) -> PeHeaderPrefix {
    PeHeaderPrefix {
        pe_offset: FileOffset::new(64),
        machine,
        number_of_sections,
        characteristics: 0x4321,
        size_of_optional_header: 2,
        kind,
    }
}

fn expected_path(index: usize, normalized: &str, key: &str, depth: u64) -> AsciiSourcePathEntry {
    AsciiSourcePathEntry {
        index,
        normalized: normalized.into(),
        key: key.into(),
        depth,
    }
}

fn mixed_owned_result() -> AsciiPeSourceHeaders {
    let names = ["Bin\\A.data", "bad", "bin/B.bin"].map(str::to_owned);
    let contents = [prefix(0x14c, 0x10b, 7), vec![], prefix(0x8664, 0x20b, 9)];
    let original_names = names.clone();
    let original_contents = contents.clone();
    let sources: Vec<_> = names
        .iter()
        .zip(&contents)
        .map(|(path, bytes)| AsciiPeSource { path, bytes })
        .collect();
    let result = parse_ascii_pe_source_headers(&sources, limits()).unwrap();
    assert_eq!(
        parse_ascii_pe_source_headers(&sources, limits()).unwrap(),
        result
    );
    assert_eq!(names, original_names);
    assert_eq!(contents, original_contents);
    result
}

#[test]
fn mixed_results_keep_exact_source_association_and_outlive_inputs() {
    let result = mixed_owned_result();
    assert_eq!(
        result,
        AsciiPeSourceHeaders {
            total_path_bytes: 22,
            total_content_bytes: 180,
            entries: vec![
                AsciiPeSourceHeader {
                    path: expected_path(0, "Bin/A.data", "bin/a.data", 2),
                    header: Ok(expected_prefix(0x14c, PeKind::Pe32, 7)),
                },
                AsciiPeSourceHeader {
                    path: expected_path(1, "bad", "bad", 1),
                    header: Err(PeHeaderError::OutOfBounds {
                        offset: FileOffset::new(0),
                        needed: 2,
                        available: 0,
                    }),
                },
                AsciiPeSourceHeader {
                    path: expected_path(2, "bin/B.bin", "bin/b.bin", 2),
                    header: Ok(expected_prefix(0x8664, PeKind::Pe32Plus, 9)),
                },
            ],
        }
    );
}

#[test]
fn reordered_records_move_both_path_and_header_with_new_indices() {
    let pe32 = prefix(0x14c, 0x10b, 7);
    let pe64 = prefix(0x8664, 0x20b, 9);
    let a = AsciiPeSource {
        path: "A.data",
        bytes: &pe32,
    };
    let b = AsciiPeSource {
        path: "B.bin",
        bytes: &pe64,
    };
    let first = parse_ascii_pe_source_headers(&[a, b], limits()).unwrap();
    let reordered = parse_ascii_pe_source_headers(&[b, a], limits()).unwrap();
    assert_eq!(reordered.entries.len(), 2);
    assert_eq!(reordered.total_content_bytes, first.total_content_bytes);
    assert_eq!(reordered.total_path_bytes, first.total_path_bytes);
    for (index, mut entry) in first.entries.into_iter().rev().enumerate() {
        entry.path.index = index;
        assert_eq!(reordered.entries[index], entry);
    }
    let paired = parse_ascii_pe_source_headers(
        &[AsciiPeSource {
            path: "A.data",
            bytes: &pe64,
        }],
        limits(),
    )
    .unwrap();
    assert_eq!(paired.entries[0].path.normalized, "A.data");
    assert_eq!(
        paired.entries[0].header,
        Ok(expected_prefix(0x8664, PeKind::Pe32Plus, 9))
    );
}

#[test]
fn aliases_charge_each_occurrence_under_distinct_paths() {
    let bytes = prefix(0x14c, 0x10b, 7);
    let sources = [
        AsciiPeSource {
            path: "a",
            bytes: &bytes,
        },
        AsciiPeSource {
            path: "b",
            bytes: &bytes,
        },
    ];
    let accepted = parse_ascii_pe_source_headers(&sources, limits()).unwrap();
    assert_eq!(accepted.total_content_bytes, 180);
    assert_eq!(accepted.total_path_bytes, 2);
    assert_eq!(accepted.entries.len(), 2);
    assert_eq!(accepted.entries[0].header, accepted.entries[1].header);
    let mut cap = limits();
    cap.headers.max_total_bytes = 179;
    assert_eq!(
        parse_ascii_pe_source_headers(&sources, cap),
        Err(AsciiPeSourceHeaderError::Headers(
            PeHeaderBatchError::TotalSizeExceeded {
                index: 1,
                total: 180,
                limit: 179
            }
        ))
    );
}

#[test]
fn all_path_admission_errors_precede_content_admission_errors() {
    let sources = [
        AsciiPeSource {
            path: "a",
            bytes: b"",
        },
        AsciiPeSource {
            path: "A",
            bytes: b"oversized",
        },
    ];
    let mut cap = limits();
    cap.paths.max_paths = 1;
    cap.headers.max_files = 0;
    cap.headers.max_file_bytes = 0;
    assert_eq!(
        parse_ascii_pe_source_headers(&sources, cap),
        Err(AsciiPeSourceHeaderError::Paths(
            AsciiSourcePathError::PathCountExceeded { count: 2, limit: 1 }
        ))
    );
    cap.paths.max_paths = 2;
    assert_eq!(
        parse_ascii_pe_source_headers(&sources, cap),
        Err(AsciiPeSourceHeaderError::Paths(
            AsciiSourcePathError::Collision {
                index: 1,
                prior: 0,
                kind: AsciiSourcePathCollision::Duplicate
            }
        ))
    );
    let sources = [
        AsciiPeSource {
            path: "",
            bytes: b"oversized",
        },
        AsciiPeSource {
            path: "long",
            bytes: b"",
        },
    ];
    cap.paths.max_path_bytes = 3;
    assert_eq!(
        parse_ascii_pe_source_headers(&sources, cap),
        Err(AsciiPeSourceHeaderError::Paths(
            AsciiSourcePathError::PathBytesExceeded {
                index: 1,
                bytes: 4,
                limit: 3
            }
        ))
    );
    cap.paths.max_path_bytes = 4;
    assert_eq!(
        parse_ascii_pe_source_headers(&sources, cap),
        Err(AsciiPeSourceHeaderError::Paths(
            AsciiSourcePathError::EmptyPath { index: 0 }
        ))
    );
}

#[test]
fn content_count_size_and_total_refusals_keep_original_indices() {
    let sources = [
        AsciiPeSource {
            path: "a",
            bytes: b"",
        },
        AsciiPeSource {
            path: "b",
            bytes: b"abcd",
        },
    ];
    let mut cap = limits();
    cap.headers.max_files = 1;
    cap.headers.max_file_bytes = 3;
    cap.headers.max_total_bytes = 3;
    assert_eq!(
        parse_ascii_pe_source_headers(&sources, cap),
        Err(AsciiPeSourceHeaderError::Headers(
            PeHeaderBatchError::FileCountExceeded { count: 2, limit: 1 }
        ))
    );
    cap.headers.max_files = 2;
    assert_eq!(
        parse_ascii_pe_source_headers(&sources, cap),
        Err(AsciiPeSourceHeaderError::Headers(
            PeHeaderBatchError::FileSizeExceeded {
                index: 1,
                size: 4,
                limit: 3
            }
        ))
    );
    cap.headers.max_file_bytes = 4;
    assert_eq!(
        parse_ascii_pe_source_headers(&sources, cap),
        Err(AsciiPeSourceHeaderError::Headers(
            PeHeaderBatchError::TotalSizeExceeded {
                index: 1,
                total: 4,
                limit: 3
            }
        ))
    );
    cap.headers.max_total_bytes = 4;
    let accepted = parse_ascii_pe_source_headers(&sources, cap).unwrap();
    assert_eq!(accepted.entries.len(), 2);
    assert_eq!(
        accepted.entries[1].header,
        Err(PeHeaderError::InvalidDosSignature {
            offset: FileOffset::new(0)
        })
    );
}

#[test]
fn every_exact_path_budget_accepts_and_one_less_refuses() {
    let sources = [AsciiPeSource {
        path: "a/b",
        bytes: b"",
    }];
    let mut cap = limits();
    cap.paths = AsciiSourcePathLimits {
        max_paths: 1,
        max_path_bytes: 3,
        max_total_path_bytes: 3,
        max_depth: 2,
    };
    cap.headers = PeHeaderBatchLimits {
        max_files: 1,
        max_file_bytes: 0,
        max_total_bytes: 0,
    };
    let accepted = parse_ascii_pe_source_headers(&sources, cap).unwrap();
    assert_eq!(accepted.total_path_bytes, 3);
    assert_eq!(accepted.total_content_bytes, 0);
    assert_eq!(accepted.entries[0].path, expected_path(0, "a/b", "a/b", 2));
    let cases = [
        (
            AsciiSourcePathLimits {
                max_paths: 0,
                ..cap.paths
            },
            AsciiSourcePathError::PathCountExceeded { count: 1, limit: 0 },
        ),
        (
            AsciiSourcePathLimits {
                max_path_bytes: 2,
                ..cap.paths
            },
            AsciiSourcePathError::PathBytesExceeded {
                index: 0,
                bytes: 3,
                limit: 2,
            },
        ),
        (
            AsciiSourcePathLimits {
                max_total_path_bytes: 2,
                ..cap.paths
            },
            AsciiSourcePathError::TotalPathBytesExceeded {
                index: 0,
                total: 3,
                limit: 2,
            },
        ),
        (
            AsciiSourcePathLimits {
                max_depth: 1,
                ..cap.paths
            },
            AsciiSourcePathError::DepthExceeded {
                index: 0,
                depth: 2,
                limit: 1,
            },
        ),
    ];
    for (paths, expected) in cases {
        assert_eq!(
            parse_ascii_pe_source_headers(&sources, AsciiPeSourceHeaderLimits { paths, ..cap }),
            Err(AsciiPeSourceHeaderError::Paths(expected))
        );
    }
}

#[test]
fn a_later_header_keeps_its_source_after_multiple_prefix_failures() {
    let bytes = prefix(0x7777, 0x20b, 3);
    let sources = [
        AsciiPeSource {
            path: "bad.exe",
            bytes: b"bad",
        },
        AsciiPeSource {
            path: "empty.dll",
            bytes: b"",
        },
        AsciiPeSource {
            path: "data",
            bytes: &bytes,
        },
    ];
    let result = parse_ascii_pe_source_headers(&sources, limits()).unwrap();
    assert_eq!(result.total_path_bytes, 20);
    assert_eq!(result.total_content_bytes, 93);
    assert_eq!(result.entries.len(), 3);
    assert_eq!(
        result.entries[0].header,
        Err(PeHeaderError::InvalidDosSignature {
            offset: FileOffset::new(0)
        })
    );
    assert_eq!(
        result.entries[1].header,
        Err(PeHeaderError::OutOfBounds {
            offset: FileOffset::new(0),
            needed: 2,
            available: 0
        })
    );
    assert_eq!(result.entries[2].path, expected_path(2, "data", "data", 1));
    assert_eq!(
        result.entries[2].header,
        Ok(expected_prefix(0x7777, PeKind::Pe32Plus, 3))
    );
}

fn generated_arithmetic_sources() -> ([std::ffi::OsString; 2], [Vec<u8>; 2]) {
    let paths = ["RING3_PE32_FIXTURE", "RING3_PE32PLUS_FIXTURE"].map(|variable| {
        std::env::var_os(variable).expect("explicit generated arithmetic fixture path")
    });
    let bytes = paths.each_ref().map(|path| std::fs::read(path).unwrap());
    assert!(bytes.iter().all(|bytes| bytes.len() == 1024));
    (paths, bytes)
}

fn generated_prefix(kind: PeKind) -> PeHeaderPrefix {
    let (machine, characteristics, size_of_optional_header) = match kind {
        PeKind::Pe32 => (0x14c, 0x103, 224),
        PeKind::Pe32Plus => (0x8664, 0x23, 240),
    };
    PeHeaderPrefix {
        pe_offset: FileOffset::new(120),
        machine,
        number_of_sections: 1,
        characteristics,
        size_of_optional_header,
        kind,
    }
}

fn generated_source_limits() -> AsciiPeSourceHeaderLimits {
    AsciiPeSourceHeaderLimits {
        paths: AsciiSourcePathLimits {
            max_paths: 3,
            max_path_bytes: 10,
            max_total_path_bytes: 25,
            max_depth: 2,
        },
        headers: PeHeaderBatchLimits {
            max_files: 3,
            max_file_bytes: 1024,
            max_total_bytes: 3072,
        },
    }
}

#[test]
#[ignore = "requires explicit RING3_PE32_FIXTURE and RING3_PE32PLUS_FIXTURE paths"]
fn generated_named_headers_preserve_both_widths_and_aliases() {
    let expected = AsciiPeSourceHeaders {
        total_path_bytes: 25,
        total_content_bytes: 3072,
        entries: vec![
            AsciiPeSourceHeader {
                path: expected_path(0, "Bin/A.data", "bin/a.data", 2),
                header: Ok(generated_prefix(PeKind::Pe32)),
            },
            AsciiPeSourceHeader {
                path: expected_path(1, "lib/B.bin", "lib/b.bin", 2),
                header: Ok(generated_prefix(PeKind::Pe32Plus)),
            },
            AsciiPeSourceHeader {
                path: expected_path(2, "copy/A", "copy/a", 2),
                header: Ok(generated_prefix(PeKind::Pe32)),
            },
        ],
    };
    let result = {
        let (paths, bytes) = generated_arithmetic_sources();
        let before = bytes.clone();
        let names = ["Bin\\A.data", "lib/B.bin", "copy/A"].map(str::to_owned);
        let original_names = names.clone();
        let sources = [
            AsciiPeSource {
                path: &names[0],
                bytes: &bytes[0],
            },
            AsciiPeSource {
                path: &names[1],
                bytes: &bytes[1],
            },
            AsciiPeSource {
                path: &names[2],
                bytes: &bytes[0],
            },
        ];
        let result = parse_ascii_pe_source_headers(&sources, generated_source_limits()).unwrap();
        assert_eq!(result, expected);
        assert_eq!(
            parse_ascii_pe_source_headers(&sources, generated_source_limits()),
            Ok(expected.clone())
        );
        assert_eq!(names, original_names);
        assert_eq!(bytes, before);
        for (path, original) in paths.iter().zip(&before) {
            assert_eq!(std::fs::read(path).unwrap(), *original);
        }
        result
    };
    assert_eq!(result, expected);
}

#[test]
#[ignore = "requires explicit RING3_PE32_FIXTURE and RING3_PE32PLUS_FIXTURE paths"]
fn generated_named_admission_refusals_preserve_priority_and_indices() {
    let (paths, bytes) = generated_arithmetic_sources();
    let before = bytes.clone();
    let mut sources = [
        AsciiPeSource {
            path: "A",
            bytes: &bytes[0],
        },
        AsciiPeSource {
            path: "a",
            bytes: &bytes[1],
        },
    ];
    let mut cap = generated_source_limits();
    cap.headers = PeHeaderBatchLimits {
        max_files: 0,
        max_file_bytes: 0,
        max_total_bytes: 0,
    };
    assert_eq!(
        parse_ascii_pe_source_headers(&sources, cap),
        Err(AsciiPeSourceHeaderError::Paths(
            AsciiSourcePathError::Collision {
                index: 1,
                prior: 0,
                kind: AsciiSourcePathCollision::Duplicate
            }
        ))
    );
    sources[1].path = "B";
    cap.headers = PeHeaderBatchLimits {
        max_files: 1,
        max_file_bytes: 1023,
        max_total_bytes: 2047,
    };
    assert_eq!(
        parse_ascii_pe_source_headers(&sources, cap),
        Err(AsciiPeSourceHeaderError::Headers(
            PeHeaderBatchError::FileCountExceeded { count: 2, limit: 1 }
        ))
    );
    cap.headers.max_files = 2;
    assert_eq!(
        parse_ascii_pe_source_headers(&sources, cap),
        Err(AsciiPeSourceHeaderError::Headers(
            PeHeaderBatchError::FileSizeExceeded {
                index: 0,
                size: 1024,
                limit: 1023
            }
        ))
    );
    cap.headers.max_file_bytes = 1024;
    assert_eq!(
        parse_ascii_pe_source_headers(&sources, cap),
        Err(AsciiPeSourceHeaderError::Headers(
            PeHeaderBatchError::TotalSizeExceeded {
                index: 1,
                total: 2048,
                limit: 2047
            }
        ))
    );
    assert_eq!(bytes, before);
    for (path, original) in paths.iter().zip(&before) {
        assert_eq!(std::fs::read(path).unwrap(), *original);
    }
}
