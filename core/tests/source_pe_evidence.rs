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

fn located_field<T>(value: T, offset: u64, byte_length: u8) -> PeFieldEvidence<T> {
    PeFieldEvidence {
        value,
        file_offset: FileOffset::new(offset),
        byte_length,
    }
}

fn compiled_pe32_evidence() -> PeDeclaredEvidence {
    PeDeclaredEvidence {
        prefix: Ok(PeHeaderPrefixEvidence {
            kind: located_field(PeKind::Pe32, 144, 2),
            machine: located_field(332, 124, 2),
            characteristics: located_field(259, 142, 2),
        }),
        optional: Ok(ring3_core::PeOptionalHeaderEvidence {
            entry_rva: located_field(ring3_core::RelativeVirtualAddress::new(4096), 160, 4),
            subsystem: located_field(3, 212, 2),
            dll_characteristics: located_field(33024, 214, 2),
            directory_count: located_field(16, 236, 4),
            clr_descriptor: Some(ring3_core::PeClrDescriptorEvidence {
                rva: located_field(ring3_core::RelativeVirtualAddress::new(0), 352, 4),
                size: located_field(0, 356, 4),
            }),
        }),
        clr: Ok(None),
    }
}

fn compiled_pe32plus_evidence() -> PeDeclaredEvidence {
    PeDeclaredEvidence {
        prefix: Ok(PeHeaderPrefixEvidence {
            kind: located_field(PeKind::Pe32Plus, 144, 2),
            machine: located_field(34404, 124, 2),
            characteristics: located_field(35, 142, 2),
        }),
        optional: Ok(ring3_core::PeOptionalHeaderEvidence {
            entry_rva: located_field(ring3_core::RelativeVirtualAddress::new(4096), 160, 4),
            subsystem: located_field(3, 212, 2),
            dll_characteristics: located_field(33056, 214, 2),
            directory_count: located_field(16, 252, 4),
            clr_descriptor: Some(ring3_core::PeClrDescriptorEvidence {
                rva: located_field(ring3_core::RelativeVirtualAddress::new(0), 368, 4),
                size: located_field(0, 372, 4),
            }),
        }),
        clr: Ok(None),
    }
}

fn compiled_managed_pe32_evidence() -> PeDeclaredEvidence {
    PeDeclaredEvidence {
        prefix: Ok(PeHeaderPrefixEvidence {
            kind: located_field(PeKind::Pe32, 152, 2),
            machine: located_field(332, 132, 2),
            characteristics: located_field(258, 150, 2),
        }),
        optional: Ok(ring3_core::PeOptionalHeaderEvidence {
            entry_rva: located_field(ring3_core::RelativeVirtualAddress::new(9078), 168, 4),
            subsystem: located_field(3, 220, 2),
            dll_characteristics: located_field(34112, 222, 2),
            directory_count: located_field(16, 244, 4),
            clr_descriptor: Some(ring3_core::PeClrDescriptorEvidence {
                rva: located_field(ring3_core::RelativeVirtualAddress::new(8200), 360, 4),
                size: located_field(72, 364, 4),
            }),
        }),
        clr: Ok(Some(ring3_core::PeClrHeaderEvidence {
            flags: located_field(3, 536, 4),
            raw_entry_point: located_field(0x0600_0001, 540, 4),
        })),
    }
}

fn compiled_managed_pe32plus_evidence() -> PeDeclaredEvidence {
    PeDeclaredEvidence {
        prefix: Ok(PeHeaderPrefixEvidence {
            kind: located_field(PeKind::Pe32Plus, 152, 2),
            machine: located_field(34404, 132, 2),
            characteristics: located_field(34, 150, 2),
        }),
        optional: Ok(ring3_core::PeOptionalHeaderEvidence {
            entry_rva: located_field(ring3_core::RelativeVirtualAddress::new(0), 168, 4),
            subsystem: located_field(3, 220, 2),
            dll_characteristics: located_field(34112, 222, 2),
            directory_count: located_field(16, 260, 4),
            clr_descriptor: Some(ring3_core::PeClrDescriptorEvidence {
                rva: located_field(ring3_core::RelativeVirtualAddress::new(8192), 376, 4),
                size: located_field(72, 380, 4),
            }),
        }),
        clr: Ok(Some(ring3_core::PeClrHeaderEvidence {
            flags: located_field(1, 528, 4),
            raw_entry_point: located_field(0x0600_0001, 532, 4),
        })),
    }
}

fn generated_evidence_sources() -> ([String; 4], [Vec<u8>; 4]) {
    let variables = [
        "RING3_PE32_FIXTURE",
        "RING3_PE32PLUS_FIXTURE",
        "RING3_CLR_PE32_FIXTURE",
        "RING3_CLR_PE32PLUS_FIXTURE",
    ];
    let paths = variables.map(|variable| std::env::var(variable).expect(variable));
    let bytes: [Vec<u8>; 4] = std::array::from_fn(|i| std::fs::read(&paths[i]).unwrap());
    assert_eq!(bytes.each_ref().map(Vec::len), [1024, 1024, 3584, 3072]);
    (paths, bytes)
}

fn generated_evidence_limits() -> AsciiPeSourceEvidenceLimits {
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
#[ignore = "requires four generated unpatched pe32/pe32+ and managed fixture paths"]
fn generated_named_evidence_preserves_mixed_pairings_and_aliases() {
    let wanted = AsciiPeSourceEvidenceBatch {
        total_path_bytes: 59,
        total_content_bytes: 9728,
        entries: vec![
            entry(0, "Bin/A.data", "bin/a.data", 2, compiled_pe32_evidence()),
            entry(1, "lib/B.bin", "lib/b.bin", 2, compiled_pe32plus_evidence()),
            entry(
                2,
                "managed/C.data",
                "managed/c.data",
                2,
                compiled_managed_pe32_evidence(),
            ),
            entry(
                3,
                "managed/D.bin",
                "managed/d.bin",
                2,
                compiled_managed_pe32plus_evidence(),
            ),
            entry(4, "copy/A", "copy/a", 2, compiled_pe32_evidence()),
            entry(5, "bad.exe", "bad.exe", 1, empty_evidence()),
        ],
    };
    let result = {
        let (fixture_paths, mut bytes) = generated_evidence_sources();
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
            inspect_ascii_pe_source_evidence(&sources, generated_evidence_limits()).unwrap();
        assert_eq!(result, wanted);
        assert_eq!(
            inspect_ascii_pe_source_evidence(&sources, generated_evidence_limits()),
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
            inspect_ascii_pe_source_evidence(&reordered, generated_evidence_limits()),
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
#[ignore = "requires four generated unpatched pe32/pe32+ and managed fixture paths"]
fn generated_named_evidence_refusals_preserve_priority_and_operands() {
    let (paths, bytes) = generated_evidence_sources();
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
    let mut cap = generated_evidence_limits();
    cap.paths.max_paths = 1;
    cap.content = PeHeaderBatchLimits {
        max_files: 0,
        max_file_bytes: 0,
        max_total_bytes: 0,
    };
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
    sources[1].path = "B";
    cap.content = PeHeaderBatchLimits {
        max_files: 1,
        max_file_bytes: 3583,
        max_total_bytes: 4607,
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
                size: 3584,
                limit: 3583
            }
        ))
    );
    cap.content.max_file_bytes = 3584;
    assert_eq!(
        inspect_ascii_pe_source_evidence(&sources, cap),
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
