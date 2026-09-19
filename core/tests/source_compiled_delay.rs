use ring3_core::{
    AsciiPeDependencyClosure, AsciiPeDependencyClosureError as ClosureError,
    AsciiPeDependencyClosureLimits, AsciiPeModuleDependencyEvidence, AsciiPeModuleDependencyLimits,
    AsciiPeModuleDependencyRequest, AsciiPeSource, AsciiPeSourceModuleEvidenceBatch,
    AsciiPeSourceModuleEvidenceLimits, AsciiSourcePathBatch, AsciiSourcePathEntry,
    AsciiSourcePathLimits, FileOffset, PeDelayImportDescriptor, PeDependencyClosureMode,
    PeDependencyRequestStep, PeDependencyRequestVisit, PeDependencyVisit, PeHeaderBatchLimits,
    PeKind, PeModuleDependencyKind, PeModuleDependencyViews, PeModuleOutputLimits,
    PeOwnedDelayImportName, PeOwnedDelayImportNameTable, RelativeVirtualAddress as Rva,
    inspect_ascii_pe_source_module_evidence, walk_ascii_pe_dependency_closure,
};

const PATHS: [&str; 2] = ["game/delayed.exe", "GAME/Ring3Delay.dll"];
const DLL: &str = "Ring3Delay.dll";

fn path_limits() -> AsciiSourcePathLimits {
    AsciiSourcePathLimits {
        max_paths: 2,
        max_path_bytes: 19,
        max_total_path_bytes: 35,
        max_depth: 2,
    }
}

fn closure_limits() -> AsciiPeDependencyClosureLimits {
    AsciiPeDependencyClosureLimits {
        observation: AsciiPeModuleDependencyLimits {
            paths: path_limits(),
            max_requests: 1,
            max_request_text_bytes: 14,
            max_basename_bytes: 14,
        },
        max_reached_sources: 2,
        max_examined_requests: 1,
    }
}

fn expected_names(kind: PeKind, ordinal: bool) -> PeOwnedDelayImportNameTable {
    let (rva, offset, name, lookup) = match kind {
        PeKind::Pe32 => (8192, 1536, if ordinal { 8268 } else { 8276 }, 8256),
        PeKind::Pe32Plus => (8200, 1544, if ordinal { 8280 } else { 8288 }, 8264),
    };
    PeOwnedDelayImportNameTable {
        kind,
        directory_rva: Rva::new(rva),
        directory_file_offset: FileOffset::new(offset),
        directory_size: 64,
        terminator_rva: Rva::new(rva + 32),
        terminator_file_offset: FileOffset::new(offset + 32),
        imports: vec![PeOwnedDelayImportName {
            descriptor: PeDelayImportDescriptor {
                descriptor_rva: Rva::new(rva),
                descriptor_file_offset: FileOffset::new(offset),
                attributes: 1,
                dll_name_address: name,
                module_handle_address: 12288,
                import_address_table_address: 12296,
                import_name_table_address: lookup,
                bound_import_address_table_address: 0,
                unload_import_address_table_address: 0,
                time_date_stamp: 0,
            },
            dll_name: DLL.into(),
        }],
    }
}

fn identities(kind: PeKind, ordinal: bool) -> [&'static str; 2] {
    match (kind, ordinal) {
        (PeKind::Pe32, false) => [
            "ab2c15214a85f8593608a30ca9a0b462ec61a04e10b4abd1e86e4869210640b8",
            "7f2adbbe2b3c039122d85f603524530eaf2f8be4fc9308ebb4a4798a22bcadbe",
        ],
        (PeKind::Pe32Plus, false) => [
            "5845501b0acf7d97c5470ce06bc738a2d2e3ce3aac64f69c164cbda8cb942e17",
            "d0dc462168a525fe4592c7950ef194eca23afef0929a753c41f2a07017f3297d",
        ],
        (PeKind::Pe32, true) => [
            "180f9f4955c325e1f550dc3e2fe13192ccaf1abae81cc8cffb40cf8eabcea391",
            "32ddf1f8f1a28c96349ff07989438e84d1cf4f6d5860ecee4b4e7b1660381bf0",
        ],
        (PeKind::Pe32Plus, true) => [
            "de7fc1f3edcc74c3ae67be3b6ca2e97bc1ae5f4ef9d51267aae35899939b80c1",
            "f88802666da0a13e8a55d90f66812410b949c9196d65c3b92108405f58bd6894",
        ],
    }
}

fn collect(
    prefix: &str,
    kind: PeKind,
    ordinal: bool,
    order: &[usize],
) -> AsciiPeSourceModuleEvidenceBatch {
    let paths = ["FIXTURE", "PROVIDER_DLL"].map(|suffix| {
        std::env::var(format!("{prefix}_{suffix}")).expect("explicit generated delay fixture path")
    });
    let mut bytes = paths.each_ref().map(|path| std::fs::read(path).unwrap());
    let lengths = bytes.each_ref().map(Vec::len);
    let sources: Vec<_> = order
        .iter()
        .map(|&original| AsciiPeSource {
            path: PATHS[original],
            bytes: &bytes[original],
        })
        .collect();
    let family = PeModuleOutputLimits {
        max_rows: 16,
        max_text_bytes: 128,
    };
    let batch = inspect_ascii_pe_source_module_evidence(
        &sources,
        AsciiPeSourceModuleEvidenceLimits {
            paths: path_limits(),
            content: PeHeaderBatchLimits {
                max_files: 2,
                max_file_bytes: 3072,
                max_total_bytes: 5120,
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
    let hashes = identities(kind, ordinal);
    for (entry, &original) in batch.entries.iter().zip(order) {
        let module = entry.module.as_ref().unwrap();
        let digest: [u8; 32] = std::array::from_fn(|index| {
            u8::from_str_radix(&hashes[original][index * 2..index * 2 + 2], 16).unwrap()
        });
        assert_eq!(module.fingerprinted.byte_length, lengths[original] as u64);
        assert_eq!(module.fingerprinted.digest, digest);
        assert_eq!(
            module.fingerprinted.evidence.prefix.unwrap().kind.value,
            kind
        );
        assert!(
            module
                .static_imports
                .as_ref()
                .unwrap()
                .descriptors
                .as_ref()
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            module
                .delay_imports
                .as_ref()
                .unwrap()
                .names
                .as_ref()
                .unwrap(),
            &if original == 0 {
                Some(expected_names(kind, ordinal))
            } else {
                None
            }
        );
    }
    assert_eq!(
        batch.total_path_bytes,
        order.iter().map(|&i| PATHS[i].len() as u64).sum()
    );
    assert_eq!(
        batch.total_content_bytes,
        order.iter().map(|&i| lengths[i] as u64).sum()
    );
    batch
}

fn expected_closure(order: &[usize], mode: PeDependencyClosureMode) -> AsciiPeDependencyClosure {
    let root = order.iter().position(|&i| i == 0).unwrap();
    let provider = order.iter().position(|&i| i == 1);
    let step = if mode == PeDependencyClosureMode::StaticOnly {
        PeDependencyRequestStep::ExcludedDelay
    } else if provider.is_some() {
        PeDependencyRequestStep::Discovered { visit_index: 1 }
    } else {
        PeDependencyRequestStep::NoCandidate
    };
    let mut visits = vec![PeDependencyVisit {
        source_index: root,
        via_request_index: None,
    }];
    if mode == PeDependencyClosureMode::StaticAndDelay
        && let Some(source_index) = provider
    {
        visits.push(PeDependencyVisit {
            source_index,
            via_request_index: Some(0),
        });
    }
    AsciiPeDependencyClosure {
        observations: AsciiPeModuleDependencyEvidence {
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
            total_requests: 1,
            total_request_text_bytes: 14,
            sources: order
                .iter()
                .map(|&original| {
                    Ok(PeModuleDependencyViews {
                        static_imports: Ok(0),
                        delay_imports: Ok(if original == 0 { Some(1) } else { None }),
                    })
                })
                .collect(),
            requests: vec![AsciiPeModuleDependencyRequest {
                source_index: root,
                kind: PeModuleDependencyKind::Delay,
                descriptor_index: 0,
                dll_name: DLL.into(),
                candidate: Ok(provider),
            }],
        },
        mode,
        visits,
        examined_requests: vec![PeDependencyRequestVisit {
            request_index: 0,
            step,
        }],
    }
}

fn verify_caps(batch: &AsciiPeSourceModuleEvidenceBatch, expected: &AsciiPeDependencyClosure) {
    let root = expected.observations.application_source_index;
    let mut caps = closure_limits();
    caps.max_examined_requests = 0;
    assert_eq!(
        walk_ascii_pe_dependency_closure(batch, root, expected.mode, caps),
        Err(ClosureError::ExaminedRequestsExceeded {
            source_index: root,
            request_index: 0,
            count: 1,
            limit: 0
        })
    );
    caps.max_reached_sources = 0;
    assert_eq!(
        walk_ascii_pe_dependency_closure(batch, root, expected.mode, caps),
        Err(ClosureError::ReachedSourcesExceeded {
            source_index: root,
            via_request_index: None,
            count: 1,
            limit: 0
        })
    );
    caps.max_examined_requests = 1;
    caps.max_reached_sources = 1;
    let result = walk_ascii_pe_dependency_closure(batch, root, expected.mode, caps);
    if let Some(visit) = expected.visits.get(1) {
        assert_eq!(
            result,
            Err(ClosureError::ReachedSourcesExceeded {
                source_index: visit.source_index,
                via_request_index: Some(0),
                count: 2,
                limit: 1,
            })
        );
    } else {
        assert_eq!(result.as_ref().unwrap(), expected);
    }
}

fn exercise(kind: PeKind, prefixes: [&str; 2]) {
    for (ordinal, prefix) in [false, true].into_iter().zip(prefixes) {
        for order in [&[0, 1][..], &[1, 0][..], &[0][..]] {
            let batch = collect(prefix, kind, ordinal, order);
            let saved = batch.clone();
            let mut retained = Vec::new();
            for mode in [
                PeDependencyClosureMode::StaticOnly,
                PeDependencyClosureMode::StaticAndDelay,
            ] {
                let expected = expected_closure(order, mode);
                let root = expected.observations.application_source_index;
                let first =
                    walk_ascii_pe_dependency_closure(&batch, root, mode, closure_limits()).unwrap();
                assert_eq!(first, expected);
                assert_eq!(
                    walk_ascii_pe_dependency_closure(&batch, root, mode, closure_limits()),
                    Ok(expected.clone())
                );
                verify_caps(&batch, &expected);
                retained.push((first, expected));
            }
            assert_eq!(batch, saved);
            drop(batch);
            drop(saved);
            for (actual, expected) in retained {
                assert_eq!(actual, expected);
            }
        }
    }
}

#[test]
#[ignore = "requires generated delay-import fixtures"]
fn compiled_pe32_delay_modes_preserve_owned_closure() {
    exercise(
        PeKind::Pe32,
        ["RING3_DELAY_PE32", "RING3_DELAY_ORDINAL_PE32"],
    );
}

#[test]
#[ignore = "requires generated delay-import fixtures"]
fn compiled_pe32plus_delay_modes_preserve_owned_closure() {
    exercise(
        PeKind::Pe32Plus,
        ["RING3_DELAY_PE32PLUS", "RING3_DELAY_ORDINAL_PE32PLUS"],
    );
}
