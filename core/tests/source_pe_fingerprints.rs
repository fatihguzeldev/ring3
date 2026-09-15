use ring3_core::{
    AsciiPeSource, AsciiPeSourceEvidenceError, AsciiPeSourceEvidenceLimits,
    AsciiPeSourceFingerprint, AsciiPeSourceFingerprintBatch, AsciiSourcePathCollision,
    AsciiSourcePathEntry, AsciiSourcePathError, AsciiSourcePathLimits, FileOffset, PeClrError,
    PeDeclaredEvidence, PeFingerprintedEvidence, PeHeaderBatchError, PeHeaderBatchLimits,
    PeHeaderError, PeRvaError, fingerprint_ascii_pe_source_evidence,
};

fn limits() -> AsciiPeSourceEvidenceLimits {
    AsciiPeSourceEvidenceLimits {
        paths: AsciiSourcePathLimits {
            max_paths: 2,
            max_path_bytes: 5,
            max_total_path_bytes: 6,
            max_depth: 2,
        },
        content: PeHeaderBatchLimits {
            max_files: 2,
            max_file_bytes: 3,
            max_total_bytes: 6,
        },
    }
}

fn expected(bytes: &[u8]) -> PeFingerprintedEvidence {
    let (hex, length) = match bytes {
        b"" => (
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            0,
        ),
        b"abc" => (
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            3,
        ),
        b"abd" => (
            "a52d159f262b2c6ddb724a61840befc36eb30c88877a4030b65cbe86298449c9",
            3,
        ),
        _ => panic!("unexpected fixture"),
    };
    let error = if bytes.is_empty() {
        PeHeaderError::OutOfBounds {
            offset: FileOffset::new(0),
            needed: 2,
            available: 0,
        }
    } else {
        PeHeaderError::InvalidDosSignature {
            offset: FileOffset::new(0),
        }
    };
    PeFingerprintedEvidence {
        byte_length: length,
        digest: std::array::from_fn(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap()),
        evidence: PeDeclaredEvidence {
            prefix: Err(error),
            optional: Err(error),
            clr: Err(PeClrError::Base(PeRvaError::Parse(error))),
        },
    }
}

fn entry(
    index: usize,
    path: &str,
    key: &str,
    depth: u64,
    bytes: &[u8],
) -> AsciiPeSourceFingerprint {
    AsciiPeSourceFingerprint {
        path: AsciiSourcePathEntry {
            index,
            normalized: path.into(),
            key: key.into(),
            depth,
        },
        fingerprint: Ok(expected(bytes)),
    }
}

#[test]
fn empty_batch_accepts_zero_limits() {
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
        fingerprint_ascii_pe_source_evidence(&[], cap),
        Ok(AsciiPeSourceFingerprintBatch {
            total_path_bytes: 0,
            total_content_bytes: 0,
            entries: vec![],
        })
    );
}

#[test]
fn mixed_owned_results_survive_reordering_repeat_and_input_destruction() {
    let wanted = AsciiPeSourceFingerprintBatch {
        total_path_bytes: 6,
        total_content_bytes: 3,
        entries: vec![
            entry(0, "BIN/A", "bin/a", 2, b"abc"),
            entry(1, "b", "b", 1, b""),
        ],
    };
    let result = {
        let mut path = String::from("BIN\\A");
        let mut content = b"abc".to_vec();
        let inputs = [
            AsciiPeSource {
                path: &path,
                bytes: &content,
            },
            AsciiPeSource {
                path: "b",
                bytes: b"",
            },
        ];
        let first = fingerprint_ascii_pe_source_evidence(&inputs, limits()).unwrap();
        assert_eq!(first, wanted);
        assert_eq!(
            fingerprint_ascii_pe_source_evidence(&inputs, limits()),
            Ok(wanted.clone())
        );
        let reverse =
            fingerprint_ascii_pe_source_evidence(&[inputs[1], inputs[0]], limits()).unwrap();
        assert_eq!(
            reverse.entries,
            vec![
                entry(0, "b", "b", 1, b""),
                entry(1, "BIN/A", "bin/a", 2, b"abc")
            ]
        );
        assert_eq!(path, "BIN\\A");
        assert_eq!(content, b"abc");
        path.replace_range(.., "destroyed");
        content.fill(0);
        drop(path);
        drop(content);
        first
    };
    assert_eq!(result, wanted);
}

#[test]
fn aliases_charge_every_occurrence_while_rebinding_changes_only_content() {
    let inputs = [
        AsciiPeSource {
            path: "a",
            bytes: b"abc",
        },
        AsciiPeSource {
            path: "b",
            bytes: b"abc",
        },
    ];
    let original = fingerprint_ascii_pe_source_evidence(&inputs, limits()).unwrap();
    assert_eq!(original.total_content_bytes, 6);
    assert_eq!(
        original.entries,
        vec![entry(0, "a", "a", 1, b"abc"), entry(1, "b", "b", 1, b"abc")]
    );
    let mut cap = limits();
    cap.content.max_total_bytes = 5;
    assert_eq!(
        fingerprint_ascii_pe_source_evidence(&inputs, cap),
        Err(AsciiPeSourceEvidenceError::Content(
            PeHeaderBatchError::TotalSizeExceeded {
                index: 1,
                total: 6,
                limit: 5
            }
        ))
    );
    let rebound = fingerprint_ascii_pe_source_evidence(
        &[AsciiPeSource {
            bytes: b"abd",
            ..inputs[0]
        }],
        limits(),
    )
    .unwrap();
    assert_eq!(rebound.entries, vec![entry(0, "a", "a", 1, b"abd")]);
    assert_eq!(
        rebound.entries[0].fingerprint.unwrap().evidence,
        original.entries[0].fingerprint.unwrap().evidence
    );
    assert_ne!(
        rebound.entries[0].fingerprint.unwrap().digest,
        original.entries[0].fingerprint.unwrap().digest
    );
}

#[test]
fn path_count_and_complete_path_validation_precede_content_refusal() {
    let inputs = [
        AsciiPeSource {
            path: "a",
            bytes: b"abc",
        },
        AsciiPeSource {
            path: "A",
            bytes: b"abcd",
        },
    ];
    let mut cap = limits();
    cap.paths.max_paths = 1;
    cap.content.max_files = 0;
    assert_eq!(
        fingerprint_ascii_pe_source_evidence(&inputs, cap),
        Err(AsciiPeSourceEvidenceError::Paths(
            AsciiSourcePathError::PathCountExceeded { count: 2, limit: 1 }
        ))
    );
    cap.paths.max_paths = 2;
    assert_eq!(
        fingerprint_ascii_pe_source_evidence(&inputs, cap),
        Err(AsciiPeSourceEvidenceError::Paths(
            AsciiSourcePathError::Collision {
                index: 1,
                prior: 0,
                kind: AsciiSourcePathCollision::Duplicate
            }
        ))
    );
    let inputs = [
        AsciiPeSource {
            path: "",
            bytes: b"abc",
        },
        AsciiPeSource {
            path: "long",
            bytes: b"",
        },
    ];
    cap.paths.max_path_bytes = 3;
    assert_eq!(
        fingerprint_ascii_pe_source_evidence(&inputs, cap),
        Err(AsciiPeSourceEvidenceError::Paths(
            AsciiSourcePathError::PathBytesExceeded {
                index: 1,
                bytes: 4,
                limit: 3
            }
        ))
    );
}

#[test]
fn content_count_size_and_total_refusals_keep_their_original_operands() {
    let inputs = [
        AsciiPeSource {
            path: "a",
            bytes: b"",
        },
        AsciiPeSource {
            path: "b",
            bytes: b"abc",
        },
    ];
    let mut cap = limits();
    cap.content = PeHeaderBatchLimits {
        max_files: 1,
        max_file_bytes: 2,
        max_total_bytes: 2,
    };
    assert_eq!(
        fingerprint_ascii_pe_source_evidence(&inputs, cap),
        Err(AsciiPeSourceEvidenceError::Content(
            PeHeaderBatchError::FileCountExceeded { count: 2, limit: 1 }
        ))
    );
    cap.content.max_files = 2;
    assert_eq!(
        fingerprint_ascii_pe_source_evidence(&inputs, cap),
        Err(AsciiPeSourceEvidenceError::Content(
            PeHeaderBatchError::FileSizeExceeded {
                index: 1,
                size: 3,
                limit: 2
            }
        ))
    );
    cap.content.max_file_bytes = 3;
    assert_eq!(
        fingerprint_ascii_pe_source_evidence(&inputs, cap),
        Err(AsciiPeSourceEvidenceError::Content(
            PeHeaderBatchError::TotalSizeExceeded {
                index: 1,
                total: 3,
                limit: 2
            }
        ))
    );
    cap.content.max_total_bytes = 3;
    assert_eq!(
        fingerprint_ascii_pe_source_evidence(&inputs, cap)
            .unwrap()
            .total_content_bytes,
        3
    );
}

#[test]
fn exact_path_limits_accept_and_one_less_refuses() {
    let inputs = [AsciiPeSource {
        path: "A/b",
        bytes: b"",
    }];
    let cap = AsciiPeSourceEvidenceLimits {
        paths: AsciiSourcePathLimits {
            max_paths: 1,
            max_path_bytes: 3,
            max_total_path_bytes: 3,
            max_depth: 2,
        },
        content: PeHeaderBatchLimits {
            max_files: 1,
            max_file_bytes: 0,
            max_total_bytes: 0,
        },
    };
    assert_eq!(
        fingerprint_ascii_pe_source_evidence(&inputs, cap)
            .unwrap()
            .entries,
        vec![entry(0, "A/b", "a/b", 2, b"")]
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
            fingerprint_ascii_pe_source_evidence(
                &inputs,
                AsciiPeSourceEvidenceLimits { paths, ..cap }
            ),
            Err(AsciiPeSourceEvidenceError::Paths(error))
        );
    }
}
