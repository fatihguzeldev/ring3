use ring3_core::{
    AsciiSourcePathBatch, AsciiSourcePathCollision as Collision, AsciiSourcePathEntry,
    AsciiSourcePathError, AsciiSourcePathLimits, AsciiSourcePathSegmentError, FileOffset,
    PeFileRange, PeFileRangeSource, PeFingerprintError, PeHeaderPrefix, PeKind, PeRvaError,
    RelativeVirtualAddress, admit_ascii_source_paths, fingerprint_pe_declared_evidence,
    parse_pe_headers, resolve_pe_file_range,
};

#[path = "../../core/tests/support/executable.rs"]
mod executable;

fn execute_image() {
    use ring3_core::execution::{Cpu32, Register32, StopReason, load_pe32};

    for (left, right) in [(7_u32, 35_u32), (100, 23), (u32::MAX, 1)] {
        let mut code = vec![0xb8];
        code.extend_from_slice(&left.to_le_bytes());
        code.push(0x05);
        code.extend_from_slice(&right.to_le_bytes());
        code.push(0xcc);
        let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        let result = cpu.run(&mut image.memory, 3);
        assert_eq!(result.reason, StopReason::Breakpoint);
        assert_eq!(result.instructions, 3);
        assert_eq!(cpu.register(Register32::Eax), left.wrapping_add(right));
    }
    let mut image = load_pe32(&executable::pe32(&[0xeb, 0xfe]), 3).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    assert_eq!(
        cpu.run(&mut image.memory, 20).reason,
        StopReason::InstructionLimit
    );
    assert_eq!(cpu.eip, image.entry_point);
}

fn fixture() -> [u8; 528] {
    let mut bytes = [0; 528];
    bytes[..2].copy_from_slice(b"MZ");
    bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
    for (offset, value) in [
        (0x84, 0x8664_u16),
        (0x86, 1),
        (0x94, 240),
        (0x96, 0x22),
        (0x98, 0x20b),
        (0xdc, 3),
    ] {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    for (offset, value) in [
        (0x3c, 0x80_u32),
        (0xa8, 0x1000),
        (0xac, 0x1000),
        (0xb8, 0x1000),
        (0xbc, 0x200),
        (0xd0, 0x2000),
        (0xd4, 0x200),
        (0x104, 16),
        (0x190, 16),
        (0x194, 0x1000),
        (0x198, 16),
        (0x19c, 0x200),
        (0x1ac, 0x4000_0040),
    ] {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes[0xb0..0xb8].copy_from_slice(&0x1_4000_0000_u64.to_le_bytes());
    bytes[0x188..0x18d].copy_from_slice(b".test");
    for (slot, value) in bytes[512..].iter_mut().zip(0x10_u8..0x20) {
        *slot = value;
    }
    bytes
}

fn inspect_image() {
    let mut bytes = fixture();
    let original = bytes;
    let headers = parse_pe_headers(&bytes).unwrap();
    assert_eq!(
        headers.prefix,
        PeHeaderPrefix {
            pe_offset: FileOffset::new(0x80),
            machine: 0x8664,
            number_of_sections: 1,
            characteristics: 0x22,
            size_of_optional_header: 240,
            kind: PeKind::Pe32Plus,
        }
    );
    assert_eq!(headers.optional.image_base, 0x1_4000_0000);
    assert_eq!(
        resolve_pe_file_range(&bytes, RelativeVirtualAddress::new(0x1003), 4),
        Ok(PeFileRange {
            file_offset: FileOffset::new(0x203),
            bytes: &[0x13, 0x14, 0x15, 0x16],
            source: PeFileRangeSource::Section(0),
        })
    );
    assert_eq!(
        resolve_pe_file_range(&bytes, RelativeVirtualAddress::new(0x100f), 2),
        Err(PeRvaError::CrossesRegionBoundary {
            start: RelativeVirtualAddress::new(0x100f),
            length: 2,
        })
    );
    assert_eq!(
        fingerprint_pe_declared_evidence(&bytes, 527),
        Err(PeFingerprintError::InputTooLarge {
            length: 528,
            limit: 527,
        })
    );
    let fingerprint = fingerprint_pe_declared_evidence(&bytes, 528).unwrap();
    assert_eq!(
        fingerprint_pe_declared_evidence(&bytes, 528),
        Ok(fingerprint)
    );
    assert_eq!(bytes, original);
    bytes.fill(0);
    assert_eq!(fingerprint.byte_length, 528);
    assert_eq!(
        fingerprint.digest,
        [
            0x56, 0x28, 0x84, 0x9c, 0x7d, 0x69, 0xb0, 0xec, 0xcb, 0x42, 0x67, 0x03, 0xc5, 0x75,
            0x5e, 0xdd, 0xf1, 0x46, 0x97, 0x09, 0x3b, 0x20, 0x91, 0xa4, 0x2c, 0x79, 0x17, 0x07,
            0xc2, 0xe7, 0xd5, 0xd1,
        ]
    );
}

fn inspect_paths() {
    let limits = AsciiSourcePathLimits {
        max_paths: 1,
        max_path_bytes: 12,
        max_total_path_bytes: 12,
        max_depth: 2,
    };
    let paths = {
        let input = String::from(r"Bin\GAME.exe");
        admit_ascii_source_paths(&[&input], limits).unwrap()
    };
    assert_eq!(
        paths,
        AsciiSourcePathBatch {
            total_path_bytes: 12,
            entries: vec![AsciiSourcePathEntry {
                index: 0,
                normalized: "Bin/GAME.exe".into(),
                key: "bin/game.exe".into(),
                depth: 2,
            }],
        }
    );
    assert_eq!(
        admit_ascii_source_paths(&["../GAME.exe"], limits),
        Err(AsciiSourcePathError::Segment {
            index: 0,
            segment: 0,
            reason: AsciiSourcePathSegmentError::Parent,
        })
    );
}

fn inspect_path_collisions() {
    let limits = AsciiSourcePathLimits {
        max_paths: 3,
        max_path_bytes: 12,
        max_total_path_bytes: 36,
        max_depth: 2,
    };
    for (paths, index, prior, kind) in [
        (vec!["Data/A", r"data\a"], 1, 0, Collision::Duplicate),
        (vec!["a", "a/b"], 1, 0, Collision::AncestorFile),
        (vec!["a/b", "a"], 1, 0, Collision::DescendantFile),
        (vec!["a!", "a/b", "a"], 2, 1, Collision::DescendantFile),
        (vec!["a/z", "a/b", "a"], 2, 1, Collision::DescendantFile),
    ] {
        assert_eq!(
            admit_ascii_source_paths(&paths, limits),
            Err(AsciiSourcePathError::Collision { index, prior, kind })
        );
    }
    assert_eq!(
        admit_ascii_source_paths(&["ab/file", "a"], limits),
        Ok(AsciiSourcePathBatch {
            total_path_bytes: 8,
            entries: vec![
                AsciiSourcePathEntry {
                    index: 0,
                    normalized: "ab/file".into(),
                    key: "ab/file".into(),
                    depth: 2,
                },
                AsciiSourcePathEntry {
                    index: 1,
                    normalized: "a".into(),
                    key: "a".into(),
                    depth: 1,
                },
            ],
        })
    );
}

fn import_fixture(name: &[u8; 6]) -> [u8; 528] {
    let mut bytes = fixture();
    for (offset, value) in [(0x110, 0x40_u32), (0x114, 40), (0x4c, 0x70)] {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes[0x70..0x76].copy_from_slice(name);
    bytes
}

fn inspect_dependency_cycle() {
    use ring3_core::{
        AsciiPeDependencyClosure, AsciiPeDependencyClosureError, AsciiPeDependencyClosureLimits,
        AsciiPeModuleDependencyEvidence, AsciiPeModuleDependencyLimits,
        AsciiPeModuleDependencyRequest, AsciiPeSource, AsciiPeSourceModuleEvidenceLimits,
        PeDependencyClosureMode, PeDependencyRequestStep, PeDependencyRequestVisit,
        PeDependencyVisit, PeHeaderBatchLimits, PeImportLookupError, PeModuleDependencyKind,
        PeModuleDependencyViews, PeModuleOutputLimits, PeOwnedImportDescriptor,
        PeStaticImportEvidence, inspect_ascii_pe_source_module_evidence,
        walk_ascii_pe_dependency_closure,
    };
    let paths = ["game/main.exe", "GAME/A.dll", "game/B.dll"];
    let names = ["A.dll", "B.dll", "A.dll"];
    let path_limits = AsciiSourcePathLimits {
        max_paths: 3,
        max_path_bytes: 13,
        max_total_path_bytes: 33,
        max_depth: 2,
    };
    let batch = {
        let mut bytes = [
            import_fixture(b"A.dll\0"),
            import_fixture(b"B.dll\0"),
            import_fixture(b"A.dll\0"),
        ];
        let sources = std::array::from_fn::<_, 3, _>(|i| AsciiPeSource {
            path: paths[i],
            bytes: &bytes[i],
        });
        let family = PeModuleOutputLimits {
            max_rows: 1,
            max_text_bytes: 5,
        };
        let batch = inspect_ascii_pe_source_module_evidence(
            &sources,
            AsciiPeSourceModuleEvidenceLimits {
                paths: path_limits,
                content: PeHeaderBatchLimits {
                    max_files: 3,
                    max_file_bytes: 528,
                    max_total_bytes: 1584,
                },
                static_imports: family,
                delay_imports: family,
                exports: family,
                bound_imports: family,
            },
        )
        .unwrap();
        for input in &mut bytes {
            input.fill(0);
        }
        batch
    };
    let digests = [
        [
            0xfc, 0x6a, 0x18, 0xb5, 0x92, 0xf4, 0x5a, 0xda, 0xe4, 0xfe, 0x55, 0x8e, 0xc9, 0x8c,
            0x8a, 0x0f, 0x90, 0x50, 0x56, 0x43, 0x87, 0xaa, 0xa9, 0xbd, 0xb2, 0xab, 0xfd, 0x5a,
            0x4b, 0x39, 0x4c, 0x5d,
        ],
        [
            0x4b, 0x4c, 0x4e, 0x22, 0x72, 0x49, 0x88, 0x51, 0xad, 0x6e, 0x31, 0xc4, 0xd3, 0x63,
            0xc4, 0x3f, 0x35, 0x5b, 0xd1, 0xf7, 0x75, 0x94, 0x76, 0xd4, 0x20, 0x31, 0x4e, 0xe8,
            0xaf, 0x2e, 0x5c, 0x8f,
        ],
    ];
    assert_eq!(batch.total_path_bytes, 33);
    assert_eq!(batch.total_content_bytes, 1584);
    assert_eq!(batch.entries.len(), 3);
    for (index, entry) in batch.entries.iter().enumerate() {
        let module = entry.module.as_ref().unwrap();
        assert_eq!(module.fingerprinted.byte_length, 528);
        assert_eq!(
            module.fingerprinted.digest,
            digests[usize::from(index == 1)]
        );
        assert_eq!(
            module.static_imports,
            Ok(PeStaticImportEvidence {
                total_rows: 1,
                total_text_bytes: 5,
                descriptors: Ok(vec![PeOwnedImportDescriptor {
                    descriptor_rva: RelativeVirtualAddress::new(64),
                    descriptor_file_offset: FileOffset::new(64),
                    import_lookup_table_rva: RelativeVirtualAddress::new(0),
                    time_date_stamp: 0,
                    forwarder_chain: 0,
                    name_rva: RelativeVirtualAddress::new(112),
                    import_address_table_rva: RelativeVirtualAddress::new(0),
                    dll_name: names[index].into(),
                }]),
                lookups: Err(PeImportLookupError::LookupTableUnavailable {
                    descriptor_index: 0
                }),
            })
        );
    }
    let limits = AsciiPeDependencyClosureLimits {
        observation: AsciiPeModuleDependencyLimits {
            paths: path_limits,
            max_requests: 3,
            max_request_text_bytes: 15,
            max_basename_bytes: 5,
        },
        max_reached_sources: 3,
        max_examined_requests: 3,
    };
    let before = batch.clone();
    let mut retained = Vec::new();
    for mode in [
        PeDependencyClosureMode::StaticOnly,
        PeDependencyClosureMode::StaticAndDelay,
    ] {
        let expected = AsciiPeDependencyClosure {
            observations: AsciiPeModuleDependencyEvidence {
                paths: AsciiSourcePathBatch {
                    total_path_bytes: 33,
                    entries: paths
                        .iter()
                        .enumerate()
                        .map(|(index, path)| AsciiSourcePathEntry {
                            index,
                            normalized: (*path).into(),
                            key: path.to_ascii_lowercase(),
                            depth: 2,
                        })
                        .collect(),
                },
                application_source_index: 0,
                total_requests: 3,
                total_request_text_bytes: 15,
                sources: vec![
                    Ok(PeModuleDependencyViews {
                        static_imports: Ok(1),
                        delay_imports: Ok(None),
                    });
                    3
                ],
                requests: [1, 2, 1]
                    .into_iter()
                    .enumerate()
                    .map(|(source_index, target)| AsciiPeModuleDependencyRequest {
                        source_index,
                        kind: PeModuleDependencyKind::Static,
                        descriptor_index: 0,
                        dll_name: names[source_index].into(),
                        candidate: Ok(Some(target)),
                    })
                    .collect(),
            },
            mode,
            visits: vec![
                PeDependencyVisit {
                    source_index: 0,
                    via_request_index: None,
                },
                PeDependencyVisit {
                    source_index: 1,
                    via_request_index: Some(0),
                },
                PeDependencyVisit {
                    source_index: 2,
                    via_request_index: Some(1),
                },
            ],
            examined_requests: vec![
                PeDependencyRequestVisit {
                    request_index: 0,
                    step: PeDependencyRequestStep::Discovered { visit_index: 1 },
                },
                PeDependencyRequestVisit {
                    request_index: 1,
                    step: PeDependencyRequestStep::Discovered { visit_index: 2 },
                },
                PeDependencyRequestVisit {
                    request_index: 2,
                    step: PeDependencyRequestStep::AlreadyReached { visit_index: 1 },
                },
            ],
        };
        assert_eq!(
            walk_ascii_pe_dependency_closure(
                &batch,
                0,
                mode,
                AsciiPeDependencyClosureLimits {
                    max_examined_requests: 2,
                    ..limits
                }
            ),
            Err(AsciiPeDependencyClosureError::ExaminedRequestsExceeded {
                source_index: 2,
                request_index: 2,
                count: 3,
                limit: 2,
            })
        );
        let actual = walk_ascii_pe_dependency_closure(&batch, 0, mode, limits).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(
            walk_ascii_pe_dependency_closure(&batch, 0, mode, limits),
            Ok(expected.clone())
        );
        retained.push((actual, expected));
    }
    assert_eq!(batch, before);
    drop(batch);
    drop(before);
    for (actual, expected) in retained {
        assert_eq!(actual, expected);
    }
}

// this isolated test cdylib owns its unique zero-argument export.
#[unsafe(no_mangle)]
pub extern "C" fn run() -> u32 {
    execute_image();
    inspect_image();
    inspect_paths();
    inspect_path_collisions();
    inspect_dependency_cycle();
    0x5233_0001
}
