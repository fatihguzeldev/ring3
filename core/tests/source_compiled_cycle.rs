use ring3_core::{
    AsciiPeDependencyClosure, AsciiPeDependencyClosureError as ClosureError,
    AsciiPeDependencyClosureLimits, AsciiPeModuleDependencyEvidence, AsciiPeModuleDependencyLimits,
    AsciiPeModuleDependencyRequest, AsciiPeSource, AsciiPeSourceModuleEvidenceBatch,
    AsciiPeSourceModuleEvidenceLimits, AsciiSourcePathBatch, AsciiSourcePathEntry,
    AsciiSourcePathLimits, FileOffset, PeDependencyClosureMode, PeDependencyRequestStep,
    PeDependencyRequestVisit, PeDependencyVisit, PeHeaderBatchLimits, PeKind,
    PeModuleDependencyKind, PeModuleDependencyViews, PeModuleOutputLimits, PeOwnedImportDescriptor,
    PeOwnedImportLookup, PeOwnedImportLookupEntry, PeOwnedImportSymbol,
    RelativeVirtualAddress as Rva, inspect_ascii_pe_source_module_evidence,
    walk_ascii_pe_dependency_closure,
};

const PATHS: [&str; 3] = [
    "game/Chain.exe",
    "GAME/Ring3Middle.dll",
    "game/Ring3Leaf.dll",
];
const DLLS: [&str; 3] = ["Ring3Middle.dll", "Ring3Leaf.dll", "Ring3Middle.dll"];
const TARGETS: [usize; 3] = [1, 2, 1];

fn path_limits() -> AsciiSourcePathLimits {
    AsciiSourcePathLimits {
        max_paths: 3,
        max_path_bytes: 20,
        max_total_path_bytes: 52,
        max_depth: 2,
    }
}

fn closure_limits() -> AsciiPeDependencyClosureLimits {
    AsciiPeDependencyClosureLimits {
        observation: AsciiPeModuleDependencyLimits {
            paths: path_limits(),
            max_requests: 3,
            max_request_text_bytes: 43,
            max_basename_bytes: 15,
        },
        max_reached_sources: 3,
        max_examined_requests: 3,
    }
}

fn expected_import(kind: PeKind, original: usize) -> PeOwnedImportLookup {
    let rows = match kind {
        PeKind::Pe32 => [
            (8192, 8232, 8264, 8240, 8248),
            (8272, 8312, 8342, 8320, 8328),
            (8268, 8308, 8340, 8316, 8324),
        ],
        PeKind::Pe32Plus => [
            (8192, 8232, 8280, 8248, 8264),
            (8272, 8312, 8358, 8328, 8344),
            (8268, 8312, 8360, 8328, 8344),
        ],
    };
    let (descriptor, lookup, name, address, hint_name) = rows[original];
    PeOwnedImportLookup {
        descriptor: PeOwnedImportDescriptor {
            descriptor_rva: Rva::new(descriptor),
            descriptor_file_offset: FileOffset::new(u64::from(descriptor - 6656)),
            import_lookup_table_rva: Rva::new(lookup),
            time_date_stamp: 0,
            forwarder_chain: 0,
            name_rva: Rva::new(name),
            import_address_table_rva: Rva::new(address),
            dll_name: DLLS[original].into(),
        },
        entries: vec![PeOwnedImportLookupEntry {
            lookup_rva: Rva::new(lookup),
            lookup_file_offset: FileOffset::new(u64::from(lookup - 6656)),
            raw_value: u64::from(hint_name),
            symbol: PeOwnedImportSymbol::ByName {
                hint_name_rva: Rva::new(hint_name),
                hint: 0,
                name: if original == 1 {
                    "ring3_leaf"
                } else {
                    "ring3_middle"
                }
                .into(),
            },
        }],
    }
}

fn collect(prefix: &str, kind: PeKind, order: &[usize]) -> AsciiPeSourceModuleEvidenceBatch {
    let digests = match kind {
        PeKind::Pe32 => [
            "8e0af322919d921344bf4787c50d20434bacf93ecc14ff6a5c79f5422828a08c",
            "06124727f3ce4f78d903754d36742dbebebf7ccce80c2e5e8e912b6e666c66d4",
            "24067a34d6ea92472f6c2d039434ebbae822f845646643cf5b6ea341171f32eb",
        ],
        PeKind::Pe32Plus => [
            "ab444f90e87f64eb90fe1b09a4008619d2c740a9e9cd8ba6e1fd2f1c76612e4d",
            "ca825ad8fb5a93184bb6779ac3a0afbc2b9875524079c40ea3c41986f715d32d",
            "b53682ee6b06060ba316eeab92626967becb00dbd6650e3165ab13064d793ef9",
        ],
    };
    let paths = ["EXE", "MIDDLE", "LEAF"].map(|suffix| {
        std::env::var(format!("{prefix}_{suffix}")).expect("explicit generated cycle fixture path")
    });
    let mut bytes = paths.each_ref().map(|path| std::fs::read(path).unwrap());
    let sources: Vec<_> = order
        .iter()
        .map(|&original| AsciiPeSource {
            path: PATHS[original],
            bytes: &bytes[original],
        })
        .collect();
    let family = PeModuleOutputLimits {
        max_rows: 3,
        max_text_bytes: 42,
    };
    let batch = inspect_ascii_pe_source_module_evidence(
        &sources,
        AsciiPeSourceModuleEvidenceLimits {
            paths: path_limits(),
            content: PeHeaderBatchLimits {
                max_files: 3,
                max_file_bytes: 2048,
                max_total_bytes: 6144,
            },
            static_imports: family,
            delay_imports: family,
            exports: family,
            bound_imports: family,
        },
    )
    .unwrap();
    drop(sources);
    for (path, buffer) in paths.iter().zip(&mut bytes) {
        assert_eq!(&std::fs::read(path).unwrap(), buffer);
        buffer.fill(0xff);
    }
    drop(bytes);
    for (entry, &original) in batch.entries.iter().zip(order) {
        let module = entry.module.as_ref().unwrap();
        let digest: [u8; 32] = std::array::from_fn(|index| {
            u8::from_str_radix(&digests[original][index * 2..index * 2 + 2], 16).unwrap()
        });
        assert_eq!(module.fingerprinted.byte_length, 2048);
        assert_eq!(module.fingerprinted.digest, digest);
        assert_eq!(
            module.fingerprinted.evidence.prefix.unwrap().kind.value,
            kind
        );
        let imports = module.static_imports.as_ref().unwrap();
        let expected = expected_import(kind, original);
        assert_eq!(
            imports.descriptors.as_ref().unwrap(),
            &vec![expected.descriptor.clone()]
        );
        assert_eq!(imports.lookups.as_ref().unwrap(), &vec![expected]);
    }
    assert_eq!(
        batch.total_path_bytes,
        order.iter().map(|&i| PATHS[i].len() as u64).sum()
    );
    assert_eq!(batch.total_content_bytes, order.len() as u64 * 2048);
    batch
}

fn expected_closure(order: &[usize], mode: PeDependencyClosureMode) -> AsciiPeDependencyClosure {
    let position = |original| order.iter().position(|&i| i == original);
    let root = position(0).unwrap();
    let middle = position(1);
    let leaf = position(2);
    let observations = AsciiPeModuleDependencyEvidence {
        paths: AsciiSourcePathBatch {
            total_path_bytes: order.iter().map(|&i| PATHS[i].len() as u64).sum(),
            entries: order
                .iter()
                .enumerate()
                .map(|(index, &original)| AsciiSourcePathEntry {
                    index,
                    normalized: PATHS[original].into(),
                    key: PATHS[original].to_ascii_lowercase(),
                    depth: 2,
                })
                .collect(),
        },
        application_source_index: root,
        total_requests: order.len() as u64,
        total_request_text_bytes: order.iter().map(|&i| DLLS[i].len() as u64).sum(),
        sources: order
            .iter()
            .map(|_| {
                Ok(PeModuleDependencyViews {
                    static_imports: Ok(1),
                    delay_imports: Ok(None),
                })
            })
            .collect(),
        requests: order
            .iter()
            .enumerate()
            .map(|(source_index, &original)| AsciiPeModuleDependencyRequest {
                source_index,
                kind: PeModuleDependencyKind::Static,
                descriptor_index: 0,
                dll_name: DLLS[original].into(),
                candidate: Ok(position(TARGETS[original])),
            })
            .collect(),
    };
    let mut visits = vec![PeDependencyVisit {
        source_index: root,
        via_request_index: None,
    }];
    let mut examined_requests = vec![PeDependencyRequestVisit {
        request_index: root,
        step: if middle.is_some() {
            PeDependencyRequestStep::Discovered { visit_index: 1 }
        } else {
            PeDependencyRequestStep::NoCandidate
        },
    }];
    if let Some(middle) = middle {
        visits.push(PeDependencyVisit {
            source_index: middle,
            via_request_index: Some(root),
        });
        examined_requests.push(PeDependencyRequestVisit {
            request_index: middle,
            step: if leaf.is_some() {
                PeDependencyRequestStep::Discovered { visit_index: 2 }
            } else {
                PeDependencyRequestStep::NoCandidate
            },
        });
        if let Some(leaf) = leaf {
            visits.push(PeDependencyVisit {
                source_index: leaf,
                via_request_index: Some(middle),
            });
            examined_requests.push(PeDependencyRequestVisit {
                request_index: leaf,
                step: PeDependencyRequestStep::AlreadyReached { visit_index: 1 },
            });
        }
    }
    AsciiPeDependencyClosure {
        observations,
        mode,
        visits,
        examined_requests,
    }
}

fn check_cycle(batch: AsciiPeSourceModuleEvidenceBatch, order: &[usize]) {
    let before = batch.clone();
    let mut retained = Vec::new();
    for mode in [
        PeDependencyClosureMode::StaticOnly,
        PeDependencyClosureMode::StaticAndDelay,
    ] {
        let expected = expected_closure(order, mode);
        let root = expected.observations.application_source_index;
        let limits = closure_limits();
        let last_request = expected.examined_requests.last().unwrap().request_index;
        let examined_count = expected.examined_requests.len() as u64;
        assert_eq!(
            walk_ascii_pe_dependency_closure(
                &batch,
                root,
                mode,
                AsciiPeDependencyClosureLimits {
                    max_examined_requests: examined_count - 1,
                    ..limits
                }
            ),
            Err(ClosureError::ExaminedRequestsExceeded {
                source_index: expected.observations.requests[last_request].source_index,
                request_index: last_request,
                count: examined_count,
                limit: examined_count - 1,
            }),
        );
        let last_visit = expected.visits.last().unwrap();
        let reached_count = expected.visits.len() as u64;
        assert_eq!(
            walk_ascii_pe_dependency_closure(
                &batch,
                root,
                mode,
                AsciiPeDependencyClosureLimits {
                    max_reached_sources: reached_count - 1,
                    ..limits
                }
            ),
            Err(ClosureError::ReachedSourcesExceeded {
                source_index: last_visit.source_index,
                via_request_index: last_visit.via_request_index,
                count: reached_count,
                limit: reached_count - 1,
            }),
        );
        let actual = walk_ascii_pe_dependency_closure(&batch, root, mode, limits).unwrap();
        assert_eq!(
            walk_ascii_pe_dependency_closure(&batch, root, mode, limits),
            Ok(actual.clone())
        );
        retained.push((actual, expected));
    }
    assert_eq!(batch, before);
    drop((batch, before));
    for (actual, expected) in retained {
        assert_eq!(actual, expected);
    }
}

fn run_cycle(prefix: &str, kind: PeKind) {
    for order in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        check_cycle(collect(prefix, kind, &order), &order);
    }
    for order in [[0, 1], [1, 0], [0, 2], [2, 0]] {
        check_cycle(collect(prefix, kind, &order), &order);
    }
}

#[test]
#[ignore = "requires explicit generated dependency-cycle fixture paths"]
fn generated_pe32_cycle_preserves_revisit_coordinates() {
    run_cycle("RING3_CYCLE_PE32", PeKind::Pe32);
}

#[test]
#[ignore = "requires explicit generated dependency-cycle fixture paths"]
fn generated_pe32plus_cycle_preserves_revisit_coordinates() {
    run_cycle("RING3_CYCLE_PE32PLUS", PeKind::Pe32Plus);
}
