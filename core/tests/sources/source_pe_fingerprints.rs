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

use ring3_core::{
    PeClrDescriptorEvidence, PeClrHeaderEvidence, PeFieldEvidence, PeHeaderPrefixEvidence, PeKind,
    PeOptionalHeaderEvidence, RelativeVirtualAddress,
};

fn compiled_field<T>(value: T, file_offset: u64, byte_length: u8) -> PeFieldEvidence<T> {
    PeFieldEvidence {
        value,
        file_offset: FileOffset::new(file_offset),
        byte_length,
    }
}

fn compiled_fingerprints() -> [PeFingerprintedEvidence; 4] {
    let digest = |hex: &str| {
        std::array::from_fn(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap())
    };
    [
        PeFingerprintedEvidence {
            byte_length: 1024,
            digest: digest("3e33f66a7f30b1cfc307e2591ad44f34148af17a7cad97f41bd8c8552ea698ac"),
            evidence: PeDeclaredEvidence {
                prefix: Ok(PeHeaderPrefixEvidence {
                    kind: compiled_field(PeKind::Pe32, 144, 2),
                    machine: compiled_field(332, 124, 2),
                    characteristics: compiled_field(259, 142, 2),
                }),
                optional: Ok(PeOptionalHeaderEvidence {
                    entry_rva: compiled_field(RelativeVirtualAddress::new(4096), 160, 4),
                    subsystem: compiled_field(3, 212, 2),
                    dll_characteristics: compiled_field(33024, 214, 2),
                    directory_count: compiled_field(16, 236, 4),
                    clr_descriptor: Some(PeClrDescriptorEvidence {
                        rva: compiled_field(RelativeVirtualAddress::new(0), 352, 4),
                        size: compiled_field(0, 356, 4),
                    }),
                }),
                clr: Ok(None),
            },
        },
        PeFingerprintedEvidence {
            byte_length: 1024,
            digest: digest("a07d0c1a4064c9b1e09682277cc2b57310f41a279cf663a9b3fa6b8a888a8e2c"),
            evidence: PeDeclaredEvidence {
                prefix: Ok(PeHeaderPrefixEvidence {
                    kind: compiled_field(PeKind::Pe32Plus, 144, 2),
                    machine: compiled_field(34404, 124, 2),
                    characteristics: compiled_field(35, 142, 2),
                }),
                optional: Ok(PeOptionalHeaderEvidence {
                    entry_rva: compiled_field(RelativeVirtualAddress::new(4096), 160, 4),
                    subsystem: compiled_field(3, 212, 2),
                    dll_characteristics: compiled_field(33056, 214, 2),
                    directory_count: compiled_field(16, 252, 4),
                    clr_descriptor: Some(PeClrDescriptorEvidence {
                        rva: compiled_field(RelativeVirtualAddress::new(0), 368, 4),
                        size: compiled_field(0, 372, 4),
                    }),
                }),
                clr: Ok(None),
            },
        },
        PeFingerprintedEvidence {
            byte_length: 3584,
            digest: digest("4315faf842c9e8badf6a5872523472ca2a8df13e035a47e56a2ba5076912c92a"),
            evidence: PeDeclaredEvidence {
                prefix: Ok(PeHeaderPrefixEvidence {
                    kind: compiled_field(PeKind::Pe32, 152, 2),
                    machine: compiled_field(332, 132, 2),
                    characteristics: compiled_field(258, 150, 2),
                }),
                optional: Ok(PeOptionalHeaderEvidence {
                    entry_rva: compiled_field(RelativeVirtualAddress::new(9078), 168, 4),
                    subsystem: compiled_field(3, 220, 2),
                    dll_characteristics: compiled_field(34112, 222, 2),
                    directory_count: compiled_field(16, 244, 4),
                    clr_descriptor: Some(PeClrDescriptorEvidence {
                        rva: compiled_field(RelativeVirtualAddress::new(8200), 360, 4),
                        size: compiled_field(72, 364, 4),
                    }),
                }),
                clr: Ok(Some(PeClrHeaderEvidence {
                    flags: compiled_field(3, 536, 4),
                    raw_entry_point: compiled_field(0x0600_0001, 540, 4),
                })),
            },
        },
        PeFingerprintedEvidence {
            byte_length: 3072,
            digest: digest("7d50f2f6db9b06867cd30e9426e96241a2e522f0704a26561cdd1525d60654a9"),
            evidence: PeDeclaredEvidence {
                prefix: Ok(PeHeaderPrefixEvidence {
                    kind: compiled_field(PeKind::Pe32Plus, 152, 2),
                    machine: compiled_field(34404, 132, 2),
                    characteristics: compiled_field(34, 150, 2),
                }),
                optional: Ok(PeOptionalHeaderEvidence {
                    entry_rva: compiled_field(RelativeVirtualAddress::new(0), 168, 4),
                    subsystem: compiled_field(3, 220, 2),
                    dll_characteristics: compiled_field(34112, 222, 2),
                    directory_count: compiled_field(16, 260, 4),
                    clr_descriptor: Some(PeClrDescriptorEvidence {
                        rva: compiled_field(RelativeVirtualAddress::new(8192), 376, 4),
                        size: compiled_field(72, 380, 4),
                    }),
                }),
                clr: Ok(Some(PeClrHeaderEvidence {
                    flags: compiled_field(1, 528, 4),
                    raw_entry_point: compiled_field(0x0600_0001, 532, 4),
                })),
            },
        },
    ]
}

fn generated_fingerprint_sources() -> ([String; 4], [Vec<u8>; 4]) {
    let variables = [
        "RING3_PE32_FIXTURE",
        "RING3_PE32PLUS_FIXTURE",
        "RING3_CLR_PE32_FIXTURE",
        "RING3_CLR_PE32PLUS_FIXTURE",
    ];
    let paths = variables.map(|variable| std::env::var(variable).expect(variable));
    let bytes = std::array::from_fn(|i| std::fs::read(&paths[i]).unwrap());
    assert_eq!(bytes.each_ref().map(Vec::len), [1024, 1024, 3584, 3072]);
    (paths, bytes)
}

fn generated_fingerprint_limits() -> AsciiPeSourceEvidenceLimits {
    AsciiPeSourceEvidenceLimits {
        paths: AsciiSourcePathLimits {
            max_paths: 6,
            max_path_bytes: 14,
            max_total_path_bytes: 59,
            max_depth: 2,
        },
        content: PeHeaderBatchLimits {
            max_files: 6,
            max_file_bytes: 3584,
            max_total_bytes: 9728,
        },
    }
}

#[test]
#[ignore = "requires four generated unpatched native and managed fixture paths"]
fn generated_named_fingerprints_preserve_mixed_pairings_and_aliases() {
    let observations = compiled_fingerprints();
    let rows = [
        ("Bin/A.data", "bin/a.data", 2, observations[0]),
        ("lib/B.bin", "lib/b.bin", 2, observations[1]),
        ("managed/C.data", "managed/c.data", 2, observations[2]),
        ("managed/D.bin", "managed/d.bin", 2, observations[3]),
        ("copy/A", "copy/a", 2, observations[0]),
        ("bad.exe", "bad.exe", 1, expected(b"")),
    ];
    let wanted = AsciiPeSourceFingerprintBatch {
        total_path_bytes: 59,
        total_content_bytes: 9728,
        entries: rows
            .into_iter()
            .enumerate()
            .map(
                |(index, (normalized, key, depth, fingerprint))| AsciiPeSourceFingerprint {
                    path: AsciiSourcePathEntry {
                        index,
                        normalized: normalized.into(),
                        key: key.into(),
                        depth,
                    },
                    fingerprint: Ok(fingerprint),
                },
            )
            .collect(),
    };
    let result = {
        let (fixture_paths, mut bytes) = generated_fingerprint_sources();
        let before = bytes.clone();
        let mut names = [
            "Bin\\A.data",
            "lib/B.bin",
            "managed/C.data",
            "managed/D.bin",
            "copy/A",
            "bad.exe",
        ]
        .map(str::to_owned);
        let original_names = names.clone();
        let sources: [AsciiPeSource<'_>; 6] = std::array::from_fn(|i| AsciiPeSource {
            path: &names[i],
            bytes: if i == 5 {
                &[]
            } else {
                &bytes[[0, 1, 2, 3, 0][i]]
            },
        });
        assert!(std::ptr::eq(sources[0].bytes, sources[4].bytes));
        let result =
            fingerprint_ascii_pe_source_evidence(&sources, generated_fingerprint_limits()).unwrap();
        assert_eq!(result, wanted);
        assert_eq!(
            fingerprint_ascii_pe_source_evidence(&sources, generated_fingerprint_limits()),
            Ok(wanted.clone())
        );
        let order = [5, 3, 1, 4, 0, 2];
        let reordered = order.map(|i| sources[i]);
        let mut expected_reordered = wanted.clone();
        expected_reordered.entries = order
            .into_iter()
            .enumerate()
            .map(|(index, prior)| {
                let mut entry = wanted.entries[prior].clone();
                entry.path.index = index;
                entry
            })
            .collect();
        assert_eq!(
            fingerprint_ascii_pe_source_evidence(&reordered, generated_fingerprint_limits()),
            Ok(expected_reordered)
        );
        assert_eq!(names, original_names);
        assert_eq!(bytes, before);
        for name in &mut names {
            name.replace_range(.., "destroyed");
        }
        for content in &mut bytes {
            content.fill(0);
        }
        drop(names);
        drop(bytes);
        for (path, original) in fixture_paths.iter().zip(&before) {
            assert_eq!(std::fs::read(path).unwrap(), *original);
        }
        result
    };
    assert_eq!(result, wanted);
}

#[test]
#[ignore = "requires four generated unpatched native and managed fixture paths"]
fn generated_named_fingerprint_refusals_preserve_priority_and_operands() {
    let (paths, bytes) = generated_fingerprint_sources();
    let before = bytes.clone();
    let mut sources = [
        AsciiPeSource {
            path: "A",
            bytes: &bytes[0],
        },
        AsciiPeSource {
            path: "a",
            bytes: &bytes[2],
        },
    ];
    let mut cap = generated_fingerprint_limits();
    cap.paths.max_paths = 1;
    cap.content = PeHeaderBatchLimits {
        max_files: 0,
        max_file_bytes: 0,
        max_total_bytes: 0,
    };
    assert_eq!(
        fingerprint_ascii_pe_source_evidence(&sources, cap),
        Err(AsciiPeSourceEvidenceError::Paths(
            AsciiSourcePathError::PathCountExceeded { count: 2, limit: 1 }
        ))
    );
    cap.paths.max_paths = 2;
    assert_eq!(
        fingerprint_ascii_pe_source_evidence(&sources, cap),
        Err(AsciiPeSourceEvidenceError::Paths(
            AsciiSourcePathError::Collision {
                index: 1,
                prior: 0,
                kind: AsciiSourcePathCollision::Duplicate
            }
        ))
    );
    sources[1].path = "B";
    cap.content = PeHeaderBatchLimits {
        max_files: 1,
        max_file_bytes: 3583,
        max_total_bytes: 4607,
    };
    assert_eq!(
        fingerprint_ascii_pe_source_evidence(&sources, cap),
        Err(AsciiPeSourceEvidenceError::Content(
            PeHeaderBatchError::FileCountExceeded { count: 2, limit: 1 }
        ))
    );
    cap.content.max_files = 2;
    assert_eq!(
        fingerprint_ascii_pe_source_evidence(&sources, cap),
        Err(AsciiPeSourceEvidenceError::Content(
            PeHeaderBatchError::FileSizeExceeded {
                index: 1,
                size: 3584,
                limit: 3583
            }
        ))
    );
    cap.content.max_file_bytes = 3584;
    assert_eq!(
        fingerprint_ascii_pe_source_evidence(&sources, cap),
        Err(AsciiPeSourceEvidenceError::Content(
            PeHeaderBatchError::TotalSizeExceeded {
                index: 1,
                total: 4608,
                limit: 4607
            }
        ))
    );
    assert_eq!(bytes, before);
    for (path, original) in paths.iter().zip(&before) {
        assert_eq!(std::fs::read(path).unwrap(), *original);
    }
}
