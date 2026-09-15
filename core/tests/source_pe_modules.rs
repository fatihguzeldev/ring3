use ring3_core::AsciiPeSourceEvidenceError::{Content, Paths};
use ring3_core::{
    AsciiPeSource, AsciiPeSourceModuleEvidence, AsciiPeSourceModuleEvidenceBatch,
    AsciiPeSourceModuleEvidenceLimits, AsciiSourcePathCollision, AsciiSourcePathEntry,
    AsciiSourcePathError, AsciiSourcePathLimits, PeDelayImportEvidenceError, PeExportEvidenceError,
    PeHeaderBatchError, PeHeaderBatchLimits, PeModuleEvidenceLimits, PeModuleOutputLimits,
    PeStaticImportEvidenceError, inspect_ascii_pe_source_module_evidence,
    inspect_pe_module_evidence,
};

fn put(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
fn word(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}
fn offset(rva: usize) -> usize {
    rva - 0x1000 + 512
}
fn text(bytes: &mut [u8], rva: usize, value: &[u8]) {
    bytes[offset(rva)..offset(rva) + value.len()].copy_from_slice(value);
}
fn slot(plus: bool) -> usize {
    152 + if plus { 112 } else { 96 }
}
fn fixture(plus: bool) -> Vec<u8> {
    let mut b = vec![0; 8192];
    b[..2].copy_from_slice(b"MZ");
    put(&mut b, 60, 128);
    b[128..132].copy_from_slice(b"PE\0\0");
    word(&mut b, 132, if plus { 0x8664 } else { 0x14c });
    word(&mut b, 134, 1);
    word(&mut b, 150, 0x2002);
    word(&mut b, 148, if plus { 240 } else { 224 });
    word(&mut b, 152, if plus { 0x20b } else { 0x10b });
    put(&mut b, 168, 0x3300);
    word(&mut b, 220, 3);
    word(&mut b, 222, 0x140);
    put(&mut b, 212, 512);
    put(&mut b, slot(plus) - 4, 16);
    for (i, rva, size) in [(0, 0x1000, 0x200), (1, 0x2000, 40), (13, 0x2800, 64)] {
        put(&mut b, slot(plus) + i * 8, rva);
        put(&mut b, slot(plus) + i * 8 + 4, size);
    }
    for (o, v) in [(8, 7680), (12, 0x1000), (16, 7680), (20, 512)] {
        put(&mut b, slot(plus) + 128 + o, v);
    }
    for (o, v) in [
        (0, 0x1234_5678),
        (4, 0xabcd_ef12),
        (12, u32::MAX),
        (16, 65534),
        (20, 3),
        (24, 2),
        (28, 0x1300),
        (32, 0x1320),
        (36, 0x1340),
    ] {
        put(&mut b, offset(0x1000) + o, v);
    }
    word(&mut b, offset(0x1000) + 8, 2);
    word(&mut b, offset(0x1000) + 10, 9);
    text(&mut b, 0x1100, b"Other.#32768\0");
    for (i, v) in [0x3300, 0, 0x1100].into_iter().enumerate() {
        put(&mut b, offset(0x1300) + i * 4, v);
    }
    for (i, (rva, index, name)) in [(0x1360, 2, b"Alias\0".as_slice()), (0x1380, 0, b"Direct\0")]
        .into_iter()
        .enumerate()
    {
        put(&mut b, offset(0x1320) + i * 4, u32::try_from(rva).unwrap());
        word(&mut b, offset(0x1340) + i * 2, index);
        text(&mut b, rva, name);
    }
    for (i, v) in [0x2200, 0x8765_4321, u32::MAX, 0x2100, 0xfeed_beef]
        .into_iter()
        .enumerate()
    {
        put(&mut b, offset(0x2000) + i * 4, v);
    }
    text(&mut b, 0x2100, b"Static.DLL\0");
    word(&mut b, offset(0x2300), 0xabcd);
    text(&mut b, 0x2302, b"Symbol\0");
    for (i, v) in [
        1,
        0x2900,
        u32::MAX,
        0xdead_beef,
        0x2a00,
        0x1234_5678,
        7,
        0x9876_5432,
    ]
    .into_iter()
    .enumerate()
    {
        put(&mut b, offset(0x2800) + i * 4, v);
    }
    text(&mut b, 0x2900, b"Delay.DLL\0");
    word(&mut b, offset(0x2b00), 0x4321);
    text(&mut b, 0x2b02, b"Later\0");
    let width = if plus { 8 } else { 4 };
    let flag = if plus { 1_u64 << 63 } else { 1 << 31 };
    for (rva, values) in [
        (0x2200, vec![0x2300, flag | 65535, 0x2300]),
        (0x2a00, vec![flag | 32768, 0x2b00]),
    ] {
        for (i, v) in values.into_iter().enumerate() {
            let o = offset(rva) + i * width;
            b[o..o + width].copy_from_slice(&v.to_le_bytes()[..width]);
        }
    }
    b
}
fn output(rows: u64, text_bytes: u64) -> PeModuleOutputLimits {
    PeModuleOutputLimits {
        max_rows: rows,
        max_text_bytes: text_bytes,
    }
}
fn limits() -> AsciiPeSourceModuleEvidenceLimits {
    AsciiPeSourceModuleEvidenceLimits {
        paths: AsciiSourcePathLimits {
            max_paths: 10,
            max_path_bytes: 100,
            max_total_path_bytes: 1000,
            max_depth: 10,
        },
        content: PeHeaderBatchLimits {
            max_files: 10,
            max_file_bytes: 8192,
            max_total_bytes: 81920,
        },
        static_imports: output(5, 32),
        delay_imports: output(5, 23),
        exports: output(8, 35),
    }
}
fn module_limits(limits: AsciiPeSourceModuleEvidenceLimits) -> PeModuleEvidenceLimits {
    PeModuleEvidenceLimits {
        max_input_bytes: limits.content.max_file_bytes,
        static_imports: limits.static_imports,
        delay_imports: limits.delay_imports,
        exports: limits.exports,
    }
}

#[test]
fn owns_ordered_mixed_results_after_path_and_content_release() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        let mut bad = b"bad".to_vec();
        let mut names = [
            "Bin\\A.data".to_owned(),
            "bad".to_owned(),
            "empty".to_owned(),
        ];
        let before = bytes.clone();
        let original_names = names.clone();
        let expected = AsciiPeSourceModuleEvidenceBatch {
            total_path_bytes: 18,
            total_content_bytes: 8195,
            entries: [bytes.as_slice(), bad.as_slice(), b""]
                .into_iter()
                .enumerate()
                .map(|(index, content)| {
                    let (normalized, key, depth) = match index {
                        0 => ("Bin/A.data", "bin/a.data", 2),
                        1 => ("bad", "bad", 1),
                        _ => ("empty", "empty", 1),
                    };
                    AsciiPeSourceModuleEvidence {
                        path: AsciiSourcePathEntry {
                            index,
                            normalized: normalized.to_owned(),
                            key: key.to_owned(),
                            depth,
                        },
                        module: inspect_pe_module_evidence(content, module_limits(limits())),
                    }
                })
                .collect(),
        };
        let owned = {
            let sources = [
                AsciiPeSource {
                    path: &names[0],
                    bytes: &bytes,
                },
                AsciiPeSource {
                    path: &names[1],
                    bytes: &bad,
                },
                AsciiPeSource {
                    path: &names[2],
                    bytes: b"",
                },
            ];
            let result = inspect_ascii_pe_source_module_evidence(&sources, limits()).unwrap();
            assert_eq!(
                result,
                inspect_ascii_pe_source_module_evidence(&sources, limits()).unwrap()
            );
            result
        };
        assert_eq!(bytes, before);
        assert_eq!(bad, b"bad");
        assert_eq!(names, original_names);
        for path in &mut names {
            path.clear();
            path.push_str("replaced");
        }
        bytes.fill(0xee);
        bad.fill(0xff);
        drop(names);
        drop(bytes);
        drop(bad);
        assert_eq!(owned, expected);
        assert!(
            owned.entries[1]
                .module
                .as_ref()
                .unwrap()
                .fingerprinted
                .evidence
                .prefix
                .is_err()
        );
        assert!(
            owned.entries[2]
                .module
                .as_ref()
                .unwrap()
                .fingerprinted
                .evidence
                .prefix
                .is_err()
        );
    }
}

#[test]
fn aliases_reordering_and_rebound_paths_preserve_each_pairing() {
    let pe32 = fixture(false);
    let pe64 = fixture(true);
    let sources = [
        AsciiPeSource {
            path: "A",
            bytes: &pe32,
        },
        AsciiPeSource {
            path: "B",
            bytes: &pe32,
        },
        AsciiPeSource {
            path: "C",
            bytes: &pe64,
        },
    ];
    let original = inspect_ascii_pe_source_module_evidence(&sources, limits()).unwrap();
    assert_eq!(original.total_content_bytes, 24576);
    assert_eq!(original.total_path_bytes, 3);
    assert_eq!(original.entries[0].module, original.entries[1].module);
    assert_ne!(original.entries[0].path, original.entries[1].path);
    let reordered =
        inspect_ascii_pe_source_module_evidence(&[sources[2], sources[0], sources[1]], limits())
            .unwrap();
    for (index, prior) in [2, 0, 1].into_iter().enumerate() {
        assert_eq!(reordered.entries[index].path.index, index);
        assert_eq!(
            reordered.entries[index].path.key,
            original.entries[prior].path.key
        );
        assert_eq!(
            reordered.entries[index].module,
            original.entries[prior].module
        );
    }
    let rebound = inspect_ascii_pe_source_module_evidence(
        &[AsciiPeSource {
            path: "A",
            bytes: &pe64,
        }],
        limits(),
    )
    .unwrap();
    assert_eq!(rebound.entries[0].path, original.entries[0].path);
    assert_eq!(rebound.entries[0].module, original.entries[2].module);
    assert_ne!(
        rebound.entries[0]
            .module
            .as_ref()
            .unwrap()
            .fingerprinted
            .digest,
        original.entries[0]
            .module
            .as_ref()
            .unwrap()
            .fingerprinted
            .digest
    );
}

#[test]
fn partial_metadata_and_empty_content_do_not_stop_later_sources() {
    for plus in [false, true] {
        let complete = fixture(plus);
        let mut partial = complete.clone();
        put(&mut partial, offset(0x2000), 0);
        put(&mut partial, offset(0x2800) + 16, 0);
        word(&mut partial, offset(0x1340), 3);
        let sources = [
            AsciiPeSource {
                path: "partial",
                bytes: &partial,
            },
            AsciiPeSource {
                path: "empty",
                bytes: b"",
            },
            AsciiPeSource {
                path: "complete",
                bytes: &complete,
            },
        ];
        let batch = inspect_ascii_pe_source_module_evidence(&sources, limits()).unwrap();
        for (entry, source) in batch.entries.iter().zip(sources) {
            assert_eq!(
                entry.module,
                inspect_pe_module_evidence(source.bytes, module_limits(limits()))
            );
        }
        let first = batch.entries[0].module.as_ref().unwrap();
        assert!(first.static_imports.as_ref().unwrap().descriptors.is_ok());
        assert!(first.static_imports.as_ref().unwrap().lookups.is_err());
        assert!(first.delay_imports.as_ref().unwrap().names.is_ok());
        assert!(first.delay_imports.as_ref().unwrap().lookups.is_err());
        assert!(first.exports.as_ref().unwrap().addresses.is_ok());
        assert!(first.exports.as_ref().unwrap().names.is_err());
        assert!(
            batch.entries[1]
                .module
                .as_ref()
                .unwrap()
                .fingerprinted
                .evidence
                .prefix
                .is_err()
        );
        assert_eq!(
            batch.entries[2].module,
            inspect_pe_module_evidence(&complete, module_limits(limits()))
        );
    }
}

#[test]
fn shared_family_limits_reset_per_occurrence_and_isolate_refusals() {
    for plus in [false, true] {
        let bytes = fixture(plus);
        let mut absent = bytes.clone();
        for directory in [0, 1, 13] {
            put(&mut absent, slot(plus) + directory * 8, 0);
            put(&mut absent, slot(plus) + directory * 8 + 4, 0);
        }
        let sources = [
            AsciiPeSource {
                path: "first",
                bytes: &bytes,
            },
            AsciiPeSource {
                path: "alias",
                bytes: &bytes,
            },
            AsciiPeSource {
                path: "later",
                bytes: &absent,
            },
        ];
        let exact = inspect_ascii_pe_source_module_evidence(&sources, limits()).unwrap();
        assert_eq!(exact.entries[0].module, exact.entries[1].module);
        for index in 0..6 {
            let mut capped = limits();
            match index {
                0 => capped.static_imports.max_rows -= 1,
                1 => capped.static_imports.max_text_bytes -= 1,
                2 => capped.delay_imports.max_rows -= 1,
                3 => capped.delay_imports.max_text_bytes -= 1,
                4 => capped.exports.max_rows -= 1,
                _ => capped.exports.max_text_bytes -= 1,
            }
            let batch = inspect_ascii_pe_source_module_evidence(&sources, capped).unwrap();
            assert_eq!(batch.entries[0].module, batch.entries[1].module);
            assert_eq!(batch.entries[2].module, exact.entries[2].module);
            let expected = exact.entries[0].module.as_ref().unwrap();
            let observed = batch.entries[0].module.as_ref().unwrap();
            assert_eq!(observed.fingerprinted, expected.fingerprinted);
            match index {
                0 | 1 => {
                    assert_eq!(observed.delay_imports, expected.delay_imports);
                    assert_eq!(observed.exports, expected.exports);
                    let error = if index == 0 {
                        PeStaticImportEvidenceError::OutputRowsExceeded { rows: 5, limit: 4 }
                    } else {
                        PeStaticImportEvidenceError::OutputTextExceeded {
                            bytes: 32,
                            limit: 31,
                        }
                    };
                    assert_eq!(observed.static_imports, Err(error));
                }
                2 | 3 => {
                    assert_eq!(observed.static_imports, expected.static_imports);
                    assert_eq!(observed.exports, expected.exports);
                    let error = if index == 2 {
                        PeDelayImportEvidenceError::OutputRowsExceeded { rows: 5, limit: 4 }
                    } else {
                        PeDelayImportEvidenceError::OutputTextExceeded {
                            bytes: 23,
                            limit: 22,
                        }
                    };
                    assert_eq!(observed.delay_imports, Err(error));
                }
                _ => {
                    assert_eq!(observed.static_imports, expected.static_imports);
                    assert_eq!(observed.delay_imports, expected.delay_imports);
                    let error = if index == 4 {
                        PeExportEvidenceError::OutputRowsExceeded { rows: 8, limit: 7 }
                    } else {
                        PeExportEvidenceError::OutputTextExceeded {
                            bytes: 35,
                            limit: 34,
                        }
                    };
                    assert_eq!(observed.exports, Err(error));
                }
            }
        }
    }
}

#[test]
fn whole_list_admission_preserves_first_errors_and_operands() {
    let bytes = fixture(false);
    let good = AsciiPeSource {
        path: "A",
        bytes: &bytes,
    };
    let empty = AsciiPeSource {
        path: "",
        bytes: b"",
    };
    let long = AsciiPeSource {
        path: "long",
        bytes: &bytes,
    };
    let mut count = limits();
    count.paths.max_paths = 1;
    count.content.max_files = 0;
    let mut path_bytes = limits();
    path_bytes.paths.max_path_bytes = 3;
    path_bytes.content.max_files = 0;
    let mut collision = limits();
    collision.content.max_files = 0;
    let mut file_count = limits();
    file_count.content.max_files = 1;
    file_count.content.max_file_bytes = 0;
    let mut file_size = limits();
    file_size.content.max_file_bytes = 8191;
    file_size.content.max_total_bytes = 0;
    let mut total = limits();
    total.content.max_total_bytes = 16383;
    let duplicate = AsciiPeSource {
        path: "a",
        bytes: b"bad",
    };
    let other = AsciiPeSource {
        path: "B",
        bytes: &bytes,
    };
    let cases = [
        (
            [empty, good],
            count,
            Paths(AsciiSourcePathError::PathCountExceeded { count: 2, limit: 1 }),
        ),
        (
            [empty, long],
            path_bytes,
            Paths(AsciiSourcePathError::PathBytesExceeded {
                index: 1,
                bytes: 4,
                limit: 3,
            }),
        ),
        (
            [good, duplicate],
            collision,
            Paths(AsciiSourcePathError::Collision {
                index: 1,
                prior: 0,
                kind: AsciiSourcePathCollision::Duplicate,
            }),
        ),
        (
            [good, other],
            file_count,
            Content(PeHeaderBatchError::FileCountExceeded { count: 2, limit: 1 }),
        ),
        (
            [
                AsciiPeSource {
                    path: "A",
                    bytes: b"",
                },
                other,
            ],
            file_size,
            Content(PeHeaderBatchError::FileSizeExceeded {
                index: 1,
                size: 8192,
                limit: 8191,
            }),
        ),
        (
            [good, other],
            total,
            Content(PeHeaderBatchError::TotalSizeExceeded {
                index: 1,
                total: 16384,
                limit: 16383,
            }),
        ),
    ];
    for (sources, caps, error) in cases {
        assert_eq!(
            inspect_ascii_pe_source_module_evidence(&sources, caps),
            Err(error)
        );
    }
}

#[test]
fn exact_and_zero_admission_limits_are_valid() {
    let mut zero = limits();
    zero.paths = AsciiSourcePathLimits {
        max_paths: 0,
        max_path_bytes: 0,
        max_total_path_bytes: 0,
        max_depth: 0,
    };
    zero.content = PeHeaderBatchLimits {
        max_files: 0,
        max_file_bytes: 0,
        max_total_bytes: 0,
    };
    zero.static_imports = output(0, 0);
    zero.delay_imports = output(0, 0);
    zero.exports = output(0, 0);
    let empty = inspect_ascii_pe_source_module_evidence(&[], zero).unwrap();
    assert_eq!(
        empty,
        AsciiPeSourceModuleEvidenceBatch {
            total_path_bytes: 0,
            total_content_bytes: 0,
            entries: Vec::new()
        }
    );
    let bytes = fixture(false);
    let sources = [AsciiPeSource {
        path: "A/b",
        bytes: &bytes,
    }];
    let mut exact = limits();
    exact.paths = AsciiSourcePathLimits {
        max_paths: 1,
        max_path_bytes: 3,
        max_total_path_bytes: 3,
        max_depth: 2,
    };
    exact.content = PeHeaderBatchLimits {
        max_files: 1,
        max_file_bytes: 8192,
        max_total_bytes: 8192,
    };
    let batch = inspect_ascii_pe_source_module_evidence(&sources, exact).unwrap();
    assert_eq!(batch.total_path_bytes, 3);
    assert_eq!(batch.total_content_bytes, 8192);
    for index in 0..7 {
        let mut below = exact;
        match index {
            0 => below.paths.max_paths -= 1,
            1 => below.paths.max_path_bytes -= 1,
            2 => below.paths.max_total_path_bytes -= 1,
            3 => below.paths.max_depth -= 1,
            4 => below.content.max_files -= 1,
            5 => below.content.max_file_bytes -= 1,
            _ => below.content.max_total_bytes -= 1,
        }
        assert!(inspect_ascii_pe_source_module_evidence(&sources, below).is_err());
    }
}
