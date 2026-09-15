use ring3_core::{
    AsciiPeSource, AsciiPeSourceEvidence, AsciiPeSourceEvidenceBatch, AsciiPeSourceEvidenceError,
    AsciiPeSourceEvidenceLimits, AsciiSourcePathCollision, AsciiSourcePathEntry,
    AsciiSourcePathError, AsciiSourcePathLimits, FileOffset, PeClrError, PeDeclaredEvidence,
    PeFieldEvidence, PeHeaderBatchError, PeHeaderBatchLimits, PeHeaderError,
    PeHeaderPrefixEvidence, PeKind, PeRvaError, inspect_ascii_pe_source_evidence,
};

fn limits() -> AsciiPeSourceEvidenceLimits {
    AsciiPeSourceEvidenceLimits {
        paths: AsciiSourcePathLimits {
            max_paths: 3,
            max_path_bytes: 10,
            max_total_path_bytes: 23,
            max_depth: 2,
        },
        content: PeHeaderBatchLimits {
            max_files: 3,
            max_file_bytes: 90,
            max_total_bytes: 180,
        },
    }
}

fn prefix(kind: PeKind) -> Vec<u8> {
    let mut bytes = vec![0; 90];
    bytes[..2].copy_from_slice(b"MZ");
    bytes[60..64].copy_from_slice(&64_u32.to_le_bytes());
    bytes[64..68].copy_from_slice(b"PE\0\0");
    bytes[68..70].copy_from_slice(&0xffff_u16.to_le_bytes());
    bytes[84..86].copy_from_slice(&2_u16.to_le_bytes());
    bytes[86..88].copy_from_slice(&0x4321_u16.to_le_bytes());
    let magic: u16 = if kind == PeKind::Pe32 { 0x10b } else { 0x20b };
    bytes[88..90].copy_from_slice(&magic.to_le_bytes());
    bytes
}

fn field<T>(value: T, offset: u64) -> PeFieldEvidence<T> {
    PeFieldEvidence {
        value,
        file_offset: FileOffset::new(offset),
        byte_length: 2,
    }
}

fn expected(kind: PeKind) -> PeDeclaredEvidence {
    let error = PeHeaderError::OptionalHeaderExtentTooShort {
        offset: FileOffset::new(88),
        required: if kind == PeKind::Pe32 { 96 } else { 112 },
        declared: 2,
    };
    PeDeclaredEvidence {
        prefix: Ok(PeHeaderPrefixEvidence {
            kind: field(kind, 88),
            machine: field(0xffff, 68),
            characteristics: field(0x4321, 86),
        }),
        optional: Err(error),
        clr: Err(PeClrError::Base(PeRvaError::Parse(error))),
    }
}

fn empty_evidence() -> PeDeclaredEvidence {
    let error = PeHeaderError::OutOfBounds {
        offset: FileOffset::new(0),
        needed: 2,
        available: 0,
    };
    PeDeclaredEvidence {
        prefix: Err(error),
        optional: Err(error),
        clr: Err(PeClrError::Base(PeRvaError::Parse(error))),
    }
}

fn entry(
    index: usize,
    normalized: &str,
    key: &str,
    depth: u64,
    evidence: PeDeclaredEvidence,
) -> AsciiPeSourceEvidence {
    AsciiPeSourceEvidence {
        path: AsciiSourcePathEntry {
            index,
            normalized: normalized.into(),
            key: key.into(),
            depth,
        },
        evidence,
    }
}

#[test]
fn empty_sources_accept_zero_limits() {
    let cap = AsciiPeSourceEvidenceLimits {
        paths: AsciiSourcePathLimits {
            max_paths: 0,
            max_path_bytes: 0,
            max_total_path_bytes: 0,
            max_depth: 0,
        },
        content: PeHeaderBatchLimits {
            max_files: 0,
            max_file_bytes: 0,
            max_total_bytes: 0,
        },
    };
    assert_eq!(
        inspect_ascii_pe_source_evidence(&[], cap),
        Ok(AsciiPeSourceEvidenceBatch {
            total_path_bytes: 0,
            total_content_bytes: 0,
            entries: vec![]
        })
    );
}

#[test]
fn mixed_results_preserve_paths_and_all_independent_reader_errors() {
    let pe32 = prefix(PeKind::Pe32);
    let plus = prefix(PeKind::Pe32Plus);
    let sources = [
        AsciiPeSource {
            path: "Bin\\A.data",
            bytes: &pe32,
        },
        AsciiPeSource {
            path: "bad",
            bytes: &[],
        },
        AsciiPeSource {
            path: "bin/B.bin",
            bytes: &plus,
        },
    ];
    assert_eq!(
        inspect_ascii_pe_source_evidence(&sources, limits()),
        Ok(AsciiPeSourceEvidenceBatch {
            total_path_bytes: 22,
            total_content_bytes: 180,
            entries: vec![
                entry(0, "Bin/A.data", "bin/a.data", 2, expected(PeKind::Pe32)),
                entry(1, "bad", "bad", 1, empty_evidence()),
                entry(2, "bin/B.bin", "bin/b.bin", 2, expected(PeKind::Pe32Plus))
            ],
        })
    );
}

#[test]
fn reordering_and_rebinding_keep_evidence_with_supplied_content() {
    let pe32 = prefix(PeKind::Pe32);
    let plus = prefix(PeKind::Pe32Plus);
    let a = AsciiPeSource {
        path: "A.data",
        bytes: &pe32,
    };
    let b = AsciiPeSource {
        path: "B.bin",
        bytes: &plus,
    };
    let forward = inspect_ascii_pe_source_evidence(&[a, b], limits()).unwrap();
    let reversed = inspect_ascii_pe_source_evidence(&[b, a], limits()).unwrap();
    assert_eq!(forward.total_content_bytes, reversed.total_content_bytes);
    assert_eq!(forward.total_path_bytes, reversed.total_path_bytes);
    assert_eq!(
        reversed.entries,
        vec![
            entry(0, "B.bin", "b.bin", 1, expected(PeKind::Pe32Plus)),
            entry(1, "A.data", "a.data", 1, expected(PeKind::Pe32))
        ]
    );
    let rebound = inspect_ascii_pe_source_evidence(
        &[AsciiPeSource {
            path: a.path,
            bytes: b.bytes,
        }],
        limits(),
    )
    .unwrap();
    assert_eq!(
        rebound.entries,
        vec![entry(0, "A.data", "a.data", 1, expected(PeKind::Pe32Plus))]
    );
}

#[test]
fn aliases_charge_every_occurrence() {
    let bytes = prefix(PeKind::Pe32);
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
    let accepted = inspect_ascii_pe_source_evidence(&sources, limits()).unwrap();
    assert_eq!(accepted.total_path_bytes, 2);
    assert_eq!(accepted.total_content_bytes, 180);
    assert_eq!(accepted.entries.len(), 2);
    assert_eq!(accepted.entries[0].evidence, accepted.entries[1].evidence);
    let mut cap = limits();
    cap.content.max_total_bytes = 179;
    assert_eq!(
        inspect_ascii_pe_source_evidence(&sources, cap),
        Err(AsciiPeSourceEvidenceError::Content(
            PeHeaderBatchError::TotalSizeExceeded {
                index: 1,
                total: 180,
                limit: 179
            }
        ))
    );
}

#[test]
fn complete_path_admission_precedes_content_errors() {
    let sources = [
        AsciiPeSource {
            path: "a",
            bytes: &[],
        },
        AsciiPeSource {
            path: "A",
            bytes: b"oversized",
        },
    ];
    let mut cap = limits();
    cap.paths.max_paths = 1;
    cap.content.max_files = 0;
    assert_eq!(
        inspect_ascii_pe_source_evidence(&sources, cap),
        Err(AsciiPeSourceEvidenceError::Paths(
            AsciiSourcePathError::PathCountExceeded { count: 2, limit: 1 }
        ))
    );
    cap.paths.max_paths = 2;
    assert_eq!(
        inspect_ascii_pe_source_evidence(&sources, cap),
        Err(AsciiPeSourceEvidenceError::Paths(
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
            bytes: &[],
        },
    ];
    cap.paths.max_path_bytes = 3;
    assert_eq!(
        inspect_ascii_pe_source_evidence(&sources, cap),
        Err(AsciiPeSourceEvidenceError::Paths(
            AsciiSourcePathError::PathBytesExceeded {
                index: 1,
                bytes: 4,
                limit: 3
            }
        ))
    );
    cap.paths.max_path_bytes = 4;
    assert_eq!(
        inspect_ascii_pe_source_evidence(&sources, cap),
        Err(AsciiPeSourceEvidenceError::Paths(
            AsciiSourcePathError::EmptyPath { index: 0 }
        ))
    );
}

#[test]
fn content_count_size_and_total_errors_keep_original_operands() {
    let sources = [
        AsciiPeSource {
            path: "a",
            bytes: &[],
        },
        AsciiPeSource {
            path: "b",
            bytes: b"abcd",
        },
    ];
    let mut cap = limits();
    cap.content = PeHeaderBatchLimits {
        max_files: 1,
        max_file_bytes: 3,
        max_total_bytes: 3,
    };
    assert_eq!(
        inspect_ascii_pe_source_evidence(&sources, cap),
        Err(AsciiPeSourceEvidenceError::Content(
            PeHeaderBatchError::FileCountExceeded { count: 2, limit: 1 }
        ))
    );
    cap.content.max_files = 2;
    assert_eq!(
        inspect_ascii_pe_source_evidence(&sources, cap),
        Err(AsciiPeSourceEvidenceError::Content(
            PeHeaderBatchError::FileSizeExceeded {
                index: 1,
                size: 4,
                limit: 3
            }
        ))
    );
    cap.content.max_file_bytes = 4;
    assert_eq!(
        inspect_ascii_pe_source_evidence(&sources, cap),
        Err(AsciiPeSourceEvidenceError::Content(
            PeHeaderBatchError::TotalSizeExceeded {
                index: 1,
                total: 4,
                limit: 3
            }
        ))
    );
    cap.content.max_total_bytes = 4;
    assert_eq!(
        inspect_ascii_pe_source_evidence(&sources, cap)
            .unwrap()
            .entries
            .len(),
        2
    );
}

#[test]
fn exact_path_limits_accept_and_each_one_less_refuses() {
    let sources = [AsciiPeSource {
        path: "A/b",
        bytes: &[],
    }];
    let mut cap = limits();
    cap.paths = AsciiSourcePathLimits {
        max_paths: 1,
        max_path_bytes: 3,
        max_total_path_bytes: 3,
        max_depth: 2,
    };
    cap.content = PeHeaderBatchLimits {
        max_files: 1,
        max_file_bytes: 0,
        max_total_bytes: 0,
    };
    assert_eq!(
        inspect_ascii_pe_source_evidence(&sources, cap)
            .unwrap()
            .entries,
        vec![entry(0, "A/b", "a/b", 2, empty_evidence())]
    );
    for (paths, error) in [
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
    ] {
        assert_eq!(
            inspect_ascii_pe_source_evidence(
                &sources,
                AsciiPeSourceEvidenceLimits { paths, ..cap }
            ),
            Err(AsciiPeSourceEvidenceError::Paths(error))
        );
    }
}

#[test]
fn owned_results_survive_both_inputs_being_overwritten_and_dropped() {
    let result = {
        let mut path = String::from("BIN\\A.bin");
        let mut bytes = prefix(PeKind::Pe32Plus);
        let original_path = path.clone();
        let original_bytes = bytes.clone();
        let sources = [AsciiPeSource {
            path: &path,
            bytes: &bytes,
        }];
        let result = inspect_ascii_pe_source_evidence(&sources, limits()).unwrap();
        assert_eq!(
            inspect_ascii_pe_source_evidence(&sources, limits()).unwrap(),
            result
        );
        assert_eq!(path, original_path);
        assert_eq!(bytes, original_bytes);
        path.replace_range(.., "destroyed");
        bytes.fill(0);
        drop(path);
        drop(bytes);
        result
    };
    assert_eq!(
        result,
        AsciiPeSourceEvidenceBatch {
            total_path_bytes: 9,
            total_content_bytes: 90,
            entries: vec![entry(
                0,
                "BIN/A.bin",
                "bin/a.bin",
                2,
                expected(PeKind::Pe32Plus)
            )]
        }
    );
}
