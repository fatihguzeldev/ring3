use ring3_core::{
    AsciiPeDependencyClosure, AsciiPeDependencyClosureError as ClosureError,
    AsciiPeDependencyClosureLimits, AsciiPeModuleDependencyEvidence, AsciiPeModuleDependencyLimits,
    AsciiPeModuleDependencyRequest, AsciiPeSource, AsciiPeSourceModuleEvidenceBatch,
    AsciiPeSourceModuleEvidenceLimits, AsciiSourcePathBatch, AsciiSourcePathEntry,
    AsciiSourcePathLimits, PeDependencyClosureMode, PeDependencyRequestStep,
    PeDependencyRequestVisit, PeDependencyVisit, PeHeaderBatchLimits, PeKind,
    PeModuleDependencyKind, PeModuleDependencyViews, PeModuleOutputLimits, PeOwnedImportSymbol,
    inspect_ascii_pe_source_module_evidence, walk_ascii_pe_dependency_closure,
};
use ring3_core::{
    FileOffset, PeExportAddressEntry, PeExportBatchError, PeExportBatchLimits,
    PeExportEvidenceLookupLimits, PeExportName, PeExportSelection, PeExportTarget,
    PeImportExportError, RelativeVirtualAddress as Rva, lookup_pe_import_evidence_exports,
};

const PATHS: [&str; 3] = [
    "game/Chain.exe",
    "GAME/Ring3Middle.dll",
    "game/Ring3Leaf.dll",
];

fn path_limits() -> AsciiSourcePathLimits {
    AsciiSourcePathLimits {
        max_paths: 3,
        max_path_bytes: 20,
        max_total_path_bytes: 52,
        max_depth: 2,
    }
}

fn collect(prefix: &str, kind: PeKind, order: &[usize]) -> AsciiPeSourceModuleEvidenceBatch {
    let digests = match kind {
        PeKind::Pe32 => [
            "8e0af322919d921344bf4787c50d20434bacf93ecc14ff6a5c79f5422828a08c",
            "06124727f3ce4f78d903754d36742dbebebf7ccce80c2e5e8e912b6e666c66d4",
            "3cab3defe7790a3106c416f3c6447b43b17dd494e9e54d17ad051728f8756311",
        ],
        PeKind::Pe32Plus => [
            "ab444f90e87f64eb90fe1b09a4008619d2c740a9e9cd8ba6e1fd2f1c76612e4d",
            "ca825ad8fb5a93184bb6779ac3a0afbc2b9875524079c40ea3c41986f715d32d",
            "ef0f6a07f4c43a987eece9b062fa93e95d92320c05e31a60c97730449c76c936",
        ],
    };
    let paths = ["EXE", "MIDDLE", "LEAF"].map(|suffix| {
        std::env::var(format!("{prefix}_{suffix}")).expect("explicit generated chain fixture path")
    });
    let bytes = paths.each_ref().map(|path| std::fs::read(path).unwrap());
    let sources: Vec<_> = order
        .iter()
        .map(|&index| AsciiPeSource {
            path: PATHS[index],
            bytes: &bytes[index],
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
    for (entry, &original) in batch.entries.iter().zip(order) {
        let module = entry.module.as_ref().unwrap();
        let expected_digest: [u8; 32] = std::array::from_fn(|index| {
            u8::from_str_radix(&digests[original][index * 2..index * 2 + 2], 16).unwrap()
        });
        assert_eq!(module.fingerprinted.byte_length, 2048);
        assert_eq!(module.fingerprinted.digest, expected_digest);
        assert_eq!(
            module.fingerprinted.evidence.prefix.unwrap().kind.value,
            kind
        );
        let lookups = module
            .static_imports
            .as_ref()
            .unwrap()
            .lookups
            .as_ref()
            .unwrap();
        if original == 2 {
            assert!(lookups.is_empty());
        } else {
            assert_eq!(lookups.len(), 1);
            assert_eq!(lookups[0].entries.len(), 1);
            let PeOwnedImportSymbol::ByName { hint, name, .. } = &lookups[0].entries[0].symbol
            else {
                panic!("chain request must use its named import");
            };
            assert_eq!(*hint, 0);
            assert_eq!(
                name,
                if original == 0 {
                    "ring3_middle"
                } else {
                    "ring3_leaf"
                }
            );
        }
    }
    for (path, before) in paths.iter().zip(&bytes) {
        assert_eq!(&std::fs::read(path).unwrap(), before);
    }
    assert_eq!(
        batch.total_path_bytes,
        if order.len() == 3 { 52 } else { 34 }
    );
    assert_eq!(
        batch.total_content_bytes,
        if order.len() == 3 { 6144 } else { 4096 }
    );
    batch
}

fn expected_observations(order: &[usize]) -> AsciiPeModuleDependencyEvidence {
    let root = order.iter().position(|&index| index == 0).unwrap();
    let middle = order.iter().position(|&index| index == 1).unwrap();
    let leaf = order.iter().position(|&index| index == 2);
    let total_path_bytes = if leaf.is_some() { 52 } else { 34 };
    AsciiPeModuleDependencyEvidence {
        paths: AsciiSourcePathBatch {
            total_path_bytes,
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
        total_requests: 2,
        total_request_text_bytes: 28,
        sources: order
            .iter()
            .map(|&original| {
                Ok(PeModuleDependencyViews {
                    static_imports: Ok(u64::from(original != 2)),
                    delay_imports: Ok(None),
                })
            })
            .collect(),
        requests: order
            .iter()
            .enumerate()
            .filter(|(_, original)| **original != 2)
            .map(|(source_index, &original)| AsciiPeModuleDependencyRequest {
                source_index,
                kind: PeModuleDependencyKind::Static,
                descriptor_index: 0,
                dll_name: if original == 0 {
                    "Ring3Middle.dll"
                } else {
                    "Ring3Leaf.dll"
                }
                .into(),
                candidate: Ok(if original == 0 { Some(middle) } else { leaf }),
            })
            .collect(),
    }
}

fn closure_limits(has_leaf: bool) -> AsciiPeDependencyClosureLimits {
    AsciiPeDependencyClosureLimits {
        observation: AsciiPeModuleDependencyLimits {
            paths: path_limits(),
            max_requests: 2,
            max_request_text_bytes: 28,
            max_basename_bytes: 15,
        },
        max_reached_sources: if has_leaf { 3 } else { 2 },
        max_examined_requests: 2,
    }
}

fn check_chain(batch: AsciiPeSourceModuleEvidenceBatch, order: &[usize]) {
    let root = order.iter().position(|&index| index == 0).unwrap();
    let middle = order.iter().position(|&index| index == 1).unwrap();
    let leaf = order.iter().position(|&index| index == 2);
    let root_request = usize::from(root > middle);
    let middle_request = 1 - root_request;
    let observations = expected_observations(order);
    let before = batch.clone();
    let mut retained = Vec::new();
    for mode in [
        PeDependencyClosureMode::StaticOnly,
        PeDependencyClosureMode::StaticAndDelay,
    ] {
        let limits = closure_limits(leaf.is_some());
        assert_eq!(
            walk_ascii_pe_dependency_closure(
                &batch,
                root,
                mode,
                AsciiPeDependencyClosureLimits {
                    max_examined_requests: 1,
                    ..limits
                }
            ),
            Err(ClosureError::ExaminedRequestsExceeded {
                source_index: middle,
                request_index: middle_request,
                count: 2,
                limit: 1,
            })
        );
        if let Some(leaf) = leaf {
            assert_eq!(
                walk_ascii_pe_dependency_closure(
                    &batch,
                    root,
                    mode,
                    AsciiPeDependencyClosureLimits {
                        max_reached_sources: 2,
                        ..limits
                    }
                ),
                Err(ClosureError::ReachedSourcesExceeded {
                    source_index: leaf,
                    via_request_index: Some(middle_request),
                    count: 3,
                    limit: 2,
                })
            );
        }
        let actual = walk_ascii_pe_dependency_closure(&batch, root, mode, limits).unwrap();
        assert_eq!(
            walk_ascii_pe_dependency_closure(&batch, root, mode, limits),
            Ok(actual.clone())
        );
        let mut visits = vec![
            PeDependencyVisit {
                source_index: root,
                via_request_index: None,
            },
            PeDependencyVisit {
                source_index: middle,
                via_request_index: Some(root_request),
            },
        ];
        if let Some(leaf) = leaf {
            visits.push(PeDependencyVisit {
                source_index: leaf,
                via_request_index: Some(middle_request),
            });
        }
        retained.push((
            actual,
            AsciiPeDependencyClosure {
                observations: observations.clone(),
                mode,
                visits,
                examined_requests: vec![
                    PeDependencyRequestVisit {
                        request_index: root_request,
                        step: PeDependencyRequestStep::Discovered { visit_index: 1 },
                    },
                    PeDependencyRequestVisit {
                        request_index: middle_request,
                        step: if leaf.is_some() {
                            PeDependencyRequestStep::Discovered { visit_index: 2 }
                        } else {
                            PeDependencyRequestStep::NoCandidate
                        },
                    },
                ],
            },
        ));
    }
    assert_eq!(batch, before);
    drop((batch, before));
    for (actual, expected) in retained {
        assert_eq!(actual, expected);
    }
}

fn run_chain(prefix: &str, kind: PeKind) {
    for order in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        check_chain(collect(prefix, kind, &order), &order);
    }
    for order in [[0, 1], [1, 0]] {
        check_chain(collect(prefix, kind, &order), &order);
    }
}

#[test]
#[ignore = "requires explicit generated dependency-chain fixture paths"]
fn generated_pe32_chain_preserves_multihop_closure() {
    run_chain("RING3_CHAIN_PE32", PeKind::Pe32);
}

#[test]
#[ignore = "requires explicit generated dependency-chain fixture paths"]
fn generated_pe32plus_chain_preserves_multihop_closure() {
    run_chain("RING3_CHAIN_PE32PLUS", PeKind::Pe32Plus);
}

fn expected_export(provider: usize) -> PeExportSelection<'static> {
    let (eat, symbol) = match provider {
        1 => (8248, "ring3_middle"),
        2 => (8246, "ring3_leaf"),
        _ => panic!("only chain DLLs export a symbol"),
    };
    PeExportSelection::Selected {
        address: PeExportAddressEntry {
            table_index: 0,
            ordinal: 1,
            entry_rva: Rva::new(eat),
            entry_file_offset: FileOffset::new(u64::from(eat - 6656)),
            target: PeExportTarget::Rva(Rva::new(4096)),
        },
        name: Some(PeExportName {
            table_index: 0,
            name_pointer_rva: Rva::new(eat + 4),
            name_pointer_file_offset: FileOffset::new(u64::from(eat - 6652)),
            ordinal_entry_rva: Rva::new(eat + 8),
            ordinal_entry_file_offset: FileOffset::new(u64::from(eat - 6648)),
            address_index: 0,
            name_rva: Rva::new(eat + 10),
            name_file_offset: FileOffset::new(u64::from(eat - 6646)),
            name: symbol,
        }),
    }
}

fn check_symbol_request(
    batch: &AsciiPeSourceModuleEvidenceBatch,
    request: &AsciiPeModuleDependencyRequest,
    order: &[usize],
) {
    let provider_index = request.candidate.unwrap().unwrap();
    let provider = order[provider_index];
    assert_eq!(provider, order[request.source_index] + 1);
    assert_eq!(request.descriptor_index, 0);
    let imports = batch.entries[request.source_index]
        .module
        .as_ref()
        .unwrap()
        .static_imports
        .as_ref()
        .unwrap();
    let expected = expected_export(provider);
    let symbol_bytes = if provider == 1 { 12 } else { 10 };
    let query_limits = PeExportEvidenceLookupLimits {
        max_table_rows: 2,
        max_table_text_bytes: symbol_bytes,
    };
    let caps = PeExportBatchLimits {
        max_queries: 1,
        max_selection_rows: 1,
    };
    let exports = batch.entries[provider_index]
        .module
        .as_ref()
        .unwrap()
        .exports
        .as_ref()
        .unwrap();
    let result = lookup_pe_import_evidence_exports(
        imports,
        u16::try_from(request.descriptor_index).unwrap(),
        exports,
        query_limits,
        caps,
    )
    .unwrap();
    let original = &imports.lookups.as_ref().unwrap()[0];
    assert!(std::ptr::eq(result.imports, original));
    assert_eq!(result.imports.descriptor.dll_name, request.dll_name);
    assert_eq!(result.exports.selection_rows, 1);
    assert_eq!(result.exports.selections, vec![Ok(expected)]);
    for (caps, expected) in [
        (
            PeExportBatchLimits {
                max_queries: 0,
                ..caps
            },
            PeExportBatchError::QueryCountExceeded { count: 1, limit: 0 },
        ),
        (
            PeExportBatchLimits {
                max_selection_rows: 0,
                ..caps
            },
            PeExportBatchError::SelectionRowsExceeded {
                index: 0,
                total: 1,
                limit: 0,
            },
        ),
    ] {
        assert_eq!(
            lookup_pe_import_evidence_exports(imports, 0, exports, query_limits, caps),
            Err(PeImportExportError::ExportBatch(expected)),
        );
    }
    for (index, &original) in order.iter().enumerate().filter(|(_, n)| **n != provider) {
        let wrong = batch.entries[index]
            .module
            .as_ref()
            .unwrap()
            .exports
            .as_ref()
            .unwrap();
        let result = lookup_pe_import_evidence_exports(
            imports,
            0,
            wrong,
            PeExportEvidenceLookupLimits {
                max_table_text_bytes: 12,
                ..query_limits
            },
            PeExportBatchLimits {
                max_selection_rows: 0,
                ..caps
            },
        )
        .unwrap();
        assert_eq!(result.exports.selection_rows, 0);
        assert_eq!(
            result.exports.selections,
            vec![Ok(if original == 0 {
                PeExportSelection::DirectoryAbsent
            } else {
                PeExportSelection::NameNotFound
            })]
        );
    }
}

fn run_chain_symbols(prefix: &str, kind: PeKind) {
    for order in [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ] {
        let batch = collect(prefix, kind, &order);
        let before = batch.clone();
        let root = order.iter().position(|&n| n == 0).unwrap();
        let closure = walk_ascii_pe_dependency_closure(
            &batch,
            root,
            PeDependencyClosureMode::StaticOnly,
            closure_limits(true),
        )
        .unwrap();
        assert_eq!(closure.observations, expected_observations(&order));
        assert_eq!(closure.examined_requests.len(), 2);
        for (edge, visit) in closure.examined_requests.iter().enumerate() {
            let request = &closure.observations.requests[visit.request_index];
            assert_eq!(order[request.source_index], edge);
            check_symbol_request(&batch, request, &order);
        }
        assert_eq!(batch, before);
    }
}

#[test]
#[ignore = "requires explicit generated dependency-chain fixture paths"]
fn generated_pe32_chain_candidates_select_retained_symbols() {
    run_chain_symbols("RING3_CHAIN_PE32", PeKind::Pe32);
}

#[test]
#[ignore = "requires explicit generated dependency-chain fixture paths"]
fn generated_pe32plus_chain_candidates_select_retained_symbols() {
    run_chain_symbols("RING3_CHAIN_PE32PLUS", PeKind::Pe32Plus);
}
