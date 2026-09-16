use ring3_core::{
    AsciiApplicationSourceCandidateLimits, AsciiPeModuleDependencyError,
    AsciiPeModuleDependencyEvidence, AsciiPeModuleDependencyLimits, AsciiPeModuleDependencyRequest,
    AsciiPeSource, AsciiPeSourceModuleEvidenceBatch, AsciiPeSourceModuleEvidenceLimits,
    AsciiSourcePathBatch, AsciiSourcePathEntry, AsciiSourcePathLimits, FileOffset,
    PeExportAddressEntry, PeExportBatchLimits, PeExportEvidenceLookupLimits, PeExportName,
    PeExportSelection, PeExportTarget, PeHeaderBatchLimits, PeModuleDependencyKind,
    PeModuleDependencyViews, PeModuleOutputLimits, RelativeVirtualAddress as Rva,
    find_ascii_application_source_candidate, inspect_ascii_pe_source_module_evidence,
    lookup_pe_delay_import_evidence_exports, lookup_pe_import_evidence_exports,
    observe_ascii_pe_module_dependencies,
};

struct RetainedScenario {
    batch: AsciiPeSourceModuleEvidenceBatch,
    candidate: AsciiSourcePathEntry,
    application_index: usize,
    importer_index: usize,
    delay: bool,
}

fn output(max_rows: u64, max_text_bytes: u64) -> PeModuleOutputLimits {
    PeModuleOutputLimits {
        max_rows,
        max_text_bytes,
    }
}

fn limits(delay: bool) -> AsciiPeSourceModuleEvidenceLimits {
    let path_limits = AsciiSourcePathLimits {
        max_paths: 4,
        max_path_bytes: 22,
        max_total_path_bytes: 72,
        max_depth: 2,
    };
    AsciiPeSourceModuleEvidenceLimits {
        paths: path_limits,
        content: PeHeaderBatchLimits {
            max_files: 4,
            max_file_bytes: if delay { 2560 } else { 2048 },
            max_total_bytes: if delay { 9216 } else { 8192 },
        },
        static_imports: output(3, 39),
        delay_imports: output(4, 33),
        exports: output(3, 11),
        bound_imports: output(0, 0),
    }
}

fn collect_scenario(
    importer: &[u8],
    provider: &[u8],
    delay: bool,
    order: [usize; 4],
) -> RetainedScenario {
    let dll = if delay {
        "Ring3Delay.dll"
    } else {
        "Ring3Probe.dll"
    };
    let labels = [
        "app/App.exe".to_owned(),
        "plugins/Requester.bin".to_owned(),
        format!("plugins/{dll}"),
        format!("APP/{dll}"),
    ];
    let mut paths = order.map(|index| labels[index].clone());
    let before = paths.clone();
    let application_index = order.iter().position(|&index| index == 0).unwrap();
    let importer_index = order.iter().position(|&index| index == 1).unwrap();
    let provider_index = order.iter().position(|&index| index == 3).unwrap();
    let limits = limits(delay);
    let (batch, candidate) = {
        let sources: [AsciiPeSource<'_>; 4] = std::array::from_fn(|index| AsciiPeSource {
            path: &paths[index],
            bytes: if order[index] < 2 { importer } else { provider },
        });
        let batch = inspect_ascii_pe_source_module_evidence(&sources, limits).unwrap();
        assert_eq!(
            batch,
            inspect_ascii_pe_source_module_evidence(&sources, limits).unwrap()
        );
        assert_eq!(batch.total_path_bytes, 72);
        assert_eq!(batch.total_content_bytes, limits.content.max_total_bytes);
        let module = batch.entries[importer_index].module.as_ref().unwrap();
        let mut token = if delay {
            module
                .delay_imports
                .as_ref()
                .unwrap()
                .lookups
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap()
                .imports[0]
                .import
                .dll_name
                .clone()
        } else {
            module
                .static_imports
                .as_ref()
                .unwrap()
                .lookups
                .as_ref()
                .unwrap()[0]
                .descriptor
                .dll_name
                .clone()
        };
        assert_eq!(token, dll);
        let caps = AsciiApplicationSourceCandidateLimits {
            paths: limits.paths,
            max_basename_bytes: 14,
        };
        let raw_paths = paths.each_ref().map(String::as_str);
        let candidate =
            find_ascii_application_source_candidate(&raw_paths, application_index, &token, caps)
                .unwrap()
                .unwrap();
        assert_eq!(
            find_ascii_application_source_candidate(&raw_paths, application_index, &token, caps),
            Ok(Some(candidate.clone()))
        );
        assert_eq!(token, dll);
        token.replace_range(.., "xxxxxxxxxxxxxx");
        drop(token);
        (batch, candidate)
    };
    assert_eq!(paths, before);
    for path in &mut paths {
        let len = path.len();
        path.clear();
        path.extend(std::iter::repeat_n('x', len));
    }
    drop((paths, before, labels));
    assert_eq!(
        candidate,
        AsciiSourcePathEntry {
            index: provider_index,
            normalized: format!("APP/{dll}"),
            key: format!("app/{}", dll.to_ascii_lowercase()),
            depth: 2,
        }
    );
    assert_eq!(candidate, batch.entries[candidate.index].path);
    RetainedScenario {
        batch,
        candidate,
        application_index,
        importer_index,
        delay,
    }
}

fn check_dependency_observations(scenario: &RetainedScenario) -> AsciiPeModuleDependencyEvidence {
    let mut input = scenario.batch.clone();
    let caps = AsciiPeModuleDependencyLimits {
        paths: limits(scenario.delay).paths,
        max_requests: 2,
        max_request_text_bytes: 28,
        max_basename_bytes: 14,
    };
    let application = scenario.application_index;
    assert_eq!(
        observe_ascii_pe_module_dependencies(
            &input,
            application,
            AsciiPeModuleDependencyLimits {
                max_requests: 1,
                ..caps
            }
        ),
        Err(AsciiPeModuleDependencyError::RequestCountExceeded { count: 2, limit: 1 })
    );
    assert_eq!(
        observe_ascii_pe_module_dependencies(
            &input,
            application,
            AsciiPeModuleDependencyLimits {
                max_request_text_bytes: 27,
                ..caps
            }
        ),
        Err(AsciiPeModuleDependencyError::RequestTextExceeded {
            bytes: 28,
            limit: 27
        })
    );
    let report = observe_ascii_pe_module_dependencies(&input, application, caps).unwrap();
    assert_eq!(
        observe_ascii_pe_module_dependencies(&input, application, caps),
        Ok(report.clone())
    );
    assert_eq!(input, scenario.batch);
    input.entries.clear();
    drop(input);
    let (dll, kind) = if scenario.delay {
        ("Ring3Delay.dll", PeModuleDependencyKind::Delay)
    } else {
        ("Ring3Probe.dll", PeModuleDependencyKind::Static)
    };
    let mut request_sources = [application, scenario.importer_index];
    request_sources.sort_unstable();
    let expected = AsciiPeModuleDependencyEvidence {
        paths: AsciiSourcePathBatch {
            total_path_bytes: 72,
            entries: scenario
                .batch
                .entries
                .iter()
                .map(|entry| entry.path.clone())
                .collect(),
        },
        application_source_index: application,
        total_requests: 2,
        total_request_text_bytes: 28,
        sources: (0..4)
            .map(|index| {
                let importer = request_sources.contains(&index);
                Ok(PeModuleDependencyViews {
                    static_imports: Ok(u64::from(importer && !scenario.delay)),
                    delay_imports: Ok((importer && scenario.delay).then_some(1)),
                })
            })
            .collect(),
        requests: request_sources
            .into_iter()
            .map(|source_index| AsciiPeModuleDependencyRequest {
                source_index,
                kind,
                descriptor_index: 0,
                dll_name: dll.into(),
                candidate: Ok(Some(scenario.candidate.index)),
            })
            .collect(),
    };
    assert_eq!(report, expected);
    report
}

fn expected_selection(symbol: &str) -> PeExportSelection<'_> {
    PeExportSelection::Selected {
        address: PeExportAddressEntry {
            table_index: 0,
            ordinal: 1,
            entry_rva: Rva::new(8247),
            entry_file_offset: FileOffset::new(1591),
            target: PeExportTarget::Rva(Rva::new(4096)),
        },
        name: Some(PeExportName {
            table_index: 0,
            name_pointer_rva: Rva::new(8251),
            name_pointer_file_offset: FileOffset::new(1595),
            ordinal_entry_rva: Rva::new(8255),
            ordinal_entry_file_offset: FileOffset::new(1599),
            address_index: 0,
            name_rva: Rva::new(8257),
            name_file_offset: FileOffset::new(1601),
            name: symbol,
        }),
    }
}

fn check_retained_association(scenario: &RetainedScenario) {
    let report = check_dependency_observations(scenario);
    let candidate_index = report
        .requests
        .iter()
        .find(|request| request.source_index == scenario.importer_index)
        .unwrap()
        .candidate
        .unwrap()
        .unwrap();
    let importer = scenario.batch.entries[scenario.importer_index]
        .module
        .as_ref()
        .unwrap();
    let provider = scenario.batch.entries[candidate_index]
        .module
        .as_ref()
        .unwrap()
        .exports
        .as_ref()
        .unwrap();
    let symbol = if scenario.delay {
        "probe"
    } else {
        "ring3_probe"
    };
    let query_limits = PeExportEvidenceLookupLimits {
        max_table_rows: 2,
        max_table_text_bytes: symbol.len() as u64,
    };
    let batch_limits = PeExportBatchLimits {
        max_queries: 1,
        max_selection_rows: 1,
    };
    let exports = if scenario.delay {
        let imports = importer.delay_imports.as_ref().unwrap();
        let result = lookup_pe_delay_import_evidence_exports(
            imports,
            0,
            provider,
            query_limits,
            batch_limits,
        )
        .unwrap();
        let original = &imports.lookups.as_ref().unwrap().as_ref().unwrap().imports[0];
        assert!(std::ptr::eq(result.imports, original));
        assert_eq!(result.imports.import.dll_name, "Ring3Delay.dll");
        assert_eq!(result.imports.entries.len(), 1);
        result.exports
    } else {
        let imports = importer.static_imports.as_ref().unwrap();
        let result =
            lookup_pe_import_evidence_exports(imports, 0, provider, query_limits, batch_limits)
                .unwrap();
        let original = &imports.lookups.as_ref().unwrap()[0];
        assert!(std::ptr::eq(result.imports, original));
        assert_eq!(result.imports.descriptor.dll_name, "Ring3Probe.dll");
        assert_eq!(result.imports.entries.len(), 1);
        result.exports
    };
    assert_eq!(exports.selection_rows, 1);
    assert_eq!(exports.selections, vec![Ok(expected_selection(symbol))]);
}

#[test]
#[ignore = "requires four explicit self-authored PE32 importer/provider fixture paths"]
fn generated_application_candidates_select_retained_static_and_delay_providers() {
    let fixture_paths = [
        "RING3_IMPORT_PE32_FIXTURE",
        "RING3_EXPORT_PE32_NAMED_DLL",
        "RING3_DELAY_PE32_FIXTURE",
        "RING3_DELAY_PE32_PROVIDER_DLL",
    ]
    .map(|name| {
        std::env::var_os(name)
            .unwrap_or_else(|| panic!("missing fixture environment variable: {name}"))
    });
    let mut bytes =
        fixture_paths.map(|path| std::fs::read(path).expect("compiled fixture must be readable"));
    assert_eq!(bytes.each_ref().map(Vec::len), [2048, 2048, 2560, 2048]);
    let before = bytes.clone();
    let mut scenarios = Vec::new();
    for (delay, importer, provider) in [(false, &bytes[0], &bytes[1]), (true, &bytes[2], &bytes[3])]
    {
        for order in [[0, 1, 2, 3], [2, 0, 3, 1]] {
            scenarios.push(collect_scenario(importer, provider, delay, order));
        }
    }
    assert_eq!(bytes, before);
    for bytes in &mut bytes {
        bytes.fill(0xee);
    }
    drop(bytes);
    drop(before);
    for scenario in &scenarios {
        check_retained_association(scenario);
        check_retained_association(scenario);
    }
}
