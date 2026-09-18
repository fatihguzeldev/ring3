use ring3_core::{
    AsciiApplicationSourceCandidateError, AsciiPeModuleDependencyError as Error,
    AsciiPeModuleDependencyLimits as Limits, AsciiPeSourceModuleEvidence,
    AsciiPeSourceModuleEvidenceBatch, AsciiSourcePathEntry, AsciiSourcePathError as PathError,
    AsciiSourcePathLimits, AsciiSourcePathSegmentError, FileOffset, PeDelayDependencyError,
    PeDelayImportDescriptor, PeDelayImportEvidence, PeDelayImportEvidenceError,
    PeDelayImportNameError, PeFingerprintError, PeImportError, PeImportLookupError, PeKind,
    PeModuleDependencyKind as Kind, PeModuleDependencyViews as Views, PeModuleEvidenceLimits,
    PeModuleOutputLimits, PeOwnedDelayImportName, PeOwnedDelayImportNameTable,
    PeOwnedImportDescriptor, PeStaticDependencyError, PeStaticImportEvidence,
    PeStaticImportEvidenceError, RelativeVirtualAddress, inspect_pe_module_evidence,
    observe_ascii_pe_module_dependencies as observe,
};

fn limits() -> Limits {
    Limits {
        paths: AsciiSourcePathLimits {
            max_paths: 8,
            max_path_bytes: 64,
            max_total_path_bytes: 256,
            max_depth: 4,
        },
        max_requests: 16,
        max_request_text_bytes: 128,
        max_basename_bytes: 64,
    }
}

fn source(path: &str, names: &[&str], delay: Option<&[&str]>) -> AsciiPeSourceModuleEvidence {
    let zero = PeModuleOutputLimits {
        max_rows: 0,
        max_text_bytes: 0,
    };
    let mut module = inspect_pe_module_evidence(
        &[],
        PeModuleEvidenceLimits {
            max_input_bytes: 0,
            static_imports: zero,
            delay_imports: zero,
            exports: zero,
            bound_imports: zero,
        },
    )
    .unwrap();
    let rva = RelativeVirtualAddress::new(0);
    let offset = FileOffset::new(0);
    module.static_imports = Ok(PeStaticImportEvidence {
        total_rows: u64::MAX,
        total_text_bytes: u64::MAX,
        descriptors: Ok(names
            .iter()
            .map(|name| PeOwnedImportDescriptor {
                descriptor_rva: rva,
                descriptor_file_offset: offset,
                import_lookup_table_rva: rva,
                time_date_stamp: 0,
                forwarder_chain: 0,
                name_rva: rva,
                import_address_table_rva: rva,
                dll_name: (*name).into(),
            })
            .collect()),
        lookups: Err(PeImportLookupError::LookupTableUnavailable {
            descriptor_index: 7,
        }),
    });
    module.delay_imports = Ok(PeDelayImportEvidence {
        total_rows: u64::MAX,
        total_text_bytes: u64::MAX,
        descriptors: Ok(None),
        lookups: Ok(None),
        names: Ok(delay.map(|names| PeOwnedDelayImportNameTable {
            kind: PeKind::Pe32,
            directory_rva: rva,
            directory_file_offset: offset,
            directory_size: 0,
            terminator_rva: rva,
            terminator_file_offset: offset,
            imports: names
                .iter()
                .map(|name| PeOwnedDelayImportName {
                    descriptor: PeDelayImportDescriptor {
                        descriptor_rva: rva,
                        descriptor_file_offset: offset,
                        attributes: 1,
                        dll_name_address: 0,
                        module_handle_address: 0,
                        import_address_table_address: 0,
                        import_name_table_address: 0,
                        bound_import_address_table_address: 0,
                        unload_import_address_table_address: 0,
                        time_date_stamp: 0,
                    },
                    dll_name: (*name).into(),
                })
                .collect(),
        })),
    });
    AsciiPeSourceModuleEvidence {
        path: AsciiSourcePathEntry {
            index: 99,
            normalized: path.into(),
            key: "forged".into(),
            depth: 99,
        },
        module: Ok(module),
    }
}

fn batch(entries: Vec<AsciiPeSourceModuleEvidence>) -> AsciiPeSourceModuleEvidenceBatch {
    AsciiPeSourceModuleEvidenceBatch {
        total_path_bytes: u64::MAX,
        total_content_bytes: u64::MAX,
        entries,
    }
}

#[test]
fn request_candidates_match_standalone_admission_across_tokens_and_limits() {
    use ring3_core::{
        AsciiApplicationSourceCandidateLimits, find_ascii_application_source_candidate,
    };
    let tokens = [
        "lib.dll",
        "LIB.DLL",
        "main.exe",
        "missing",
        "",
        ".",
        "..",
        "CON.dll",
        "a.",
        "a ",
        "/lib.dll",
        "C:lib.dll",
        "é",
        "dir/lib.dll",
        r"dir\lib.dll",
        "%2f.dll",
    ];
    let input = batch(vec![
        source(r"App\main.exe", &tokens, Some(&tokens)),
        source("plugins/lib.dll", &[], None),
        source("APP/LIB.DLL", &[], None),
        source("App/%2f.dll", &[], None),
    ]);
    let before = input.clone();
    let labels: Vec<_> = input
        .entries
        .iter()
        .map(|e| e.path.normalized.as_str())
        .collect();
    for application in 0..input.entries.len() {
        for basename_limit in [0, 1, 7, 8, 64] {
            let caps = Limits {
                max_requests: 32,
                max_request_text_bytes: 1024,
                max_basename_bytes: basename_limit,
                ..limits()
            };
            let result = observe(&input, application, caps).unwrap();
            assert_eq!(result.requests.len(), 32);
            for (index, request) in result.requests.iter().enumerate() {
                let token = tokens[index % tokens.len()];
                let expected = find_ascii_application_source_candidate(
                    &labels,
                    application,
                    token,
                    AsciiApplicationSourceCandidateLimits {
                        paths: caps.paths,
                        max_basename_bytes: basename_limit,
                    },
                )
                .map(|entry| entry.map(|entry| entry.index));
                assert_eq!(request.source_index, 0);
                assert_eq!(request.descriptor_index, index % tokens.len());
                assert_eq!(
                    request.kind,
                    if index < tokens.len() {
                        Kind::Static
                    } else {
                        Kind::Delay
                    }
                );
                assert_eq!(request.dll_name, token);
                assert_eq!(
                    request.candidate, expected,
                    "application={application} token={token:?} cap={basename_limit}"
                );
            }
        }
    }
    assert_eq!(input, before);
}

#[test]
fn ordered_requests_own_text_and_use_the_application_context() {
    let output = {
        let mut input = batch(vec![
            source(r"App\main.exe", &[], None),
            source("plugins/requester", &["a.dll", "A.DLL"], Some(&["a.dll"])),
            source("plugins/a.dll", &[], None),
            source("app/A.DLL", &[], None),
        ]);
        let before = input.clone();
        let output = observe(&input, 0, limits()).unwrap();
        assert_eq!(input, before);
        assert_eq!(observe(&input, 0, limits()), Ok(output.clone()));
        for entry in &mut input.entries {
            entry.path.normalized.replace_range(.., "changed");
        }
        let module = input.entries[1].module.as_mut().unwrap();
        module
            .static_imports
            .as_mut()
            .unwrap()
            .descriptors
            .as_mut()
            .unwrap()[0]
            .dll_name
            .replace_range(.., "changed");
        module
            .delay_imports
            .as_mut()
            .unwrap()
            .names
            .as_mut()
            .unwrap()
            .as_mut()
            .unwrap()
            .imports[0]
            .dll_name
            .replace_range(.., "changed");
        output
    };
    assert_eq!(output.application_source_index, 0);
    assert_eq!(
        (output.total_requests, output.total_request_text_bytes),
        (3, 15)
    );
    assert_eq!(output.paths.total_path_bytes, 51);
    assert_eq!(
        output.paths.entries[0],
        AsciiSourcePathEntry {
            index: 0,
            normalized: "App/main.exe".into(),
            key: "app/main.exe".into(),
            depth: 2,
        }
    );
    assert_eq!(
        output.sources,
        vec![
            Ok(Views {
                static_imports: Ok(0),
                delay_imports: Ok(None)
            }),
            Ok(Views {
                static_imports: Ok(2),
                delay_imports: Ok(Some(1))
            }),
            Ok(Views {
                static_imports: Ok(0),
                delay_imports: Ok(None)
            }),
            Ok(Views {
                static_imports: Ok(0),
                delay_imports: Ok(None)
            }),
        ]
    );
    assert_eq!(
        output
            .requests
            .iter()
            .map(|r| (
                r.source_index,
                r.kind,
                r.descriptor_index,
                r.dll_name.as_str(),
                r.candidate
            ))
            .collect::<Vec<_>>(),
        vec![
            (1, Kind::Static, 0, "a.dll", Ok(Some(3))),
            (1, Kind::Static, 1, "A.DLL", Ok(Some(3))),
            (1, Kind::Delay, 0, "a.dll", Ok(Some(3))),
        ]
    );
}

#[test]
fn module_family_and_view_errors_remain_independent() {
    let fingerprint = PeFingerprintError::InputTooLarge {
        length: 9,
        limit: 2,
    };
    let static_error = PeStaticImportEvidenceError::OutputRowsExceeded { rows: 5, limit: 1 };
    let delay_error = PeDelayImportEvidenceError::OutputTextExceeded { bytes: 9, limit: 2 };
    let descriptor_error = PeImportError::MissingImportTerminator {
        descriptor_index: 7,
    };
    let name_error = PeDelayImportNameError::UnsupportedAttributes {
        descriptor_index: 9,
        attributes: 3,
    };
    let mut input = batch(
        (0..6)
            .map(|i| source(&format!("file{i}"), &[], None))
            .collect(),
    );
    input.entries[0].module = Err(fingerprint);
    input.entries[1] = source("file1", &[], Some(&["file5"]));
    input.entries[1].module.as_mut().unwrap().static_imports = Err(static_error);
    let third = input.entries[2].module.as_mut().unwrap();
    third.static_imports.as_mut().unwrap().descriptors = Err(descriptor_error);
    third.delay_imports = Err(delay_error);
    input.entries[3]
        .module
        .as_mut()
        .unwrap()
        .delay_imports
        .as_mut()
        .unwrap()
        .names = Err(name_error);
    input.entries[4] = source("file4", &[], Some(&[]));
    let output = observe(&input, 0, limits()).unwrap();
    assert_eq!(
        output.sources,
        vec![
            Err(fingerprint),
            Ok(Views {
                static_imports: Err(PeStaticDependencyError::Evidence(static_error)),
                delay_imports: Ok(Some(1))
            }),
            Ok(Views {
                static_imports: Err(PeStaticDependencyError::Descriptors(descriptor_error)),
                delay_imports: Err(PeDelayDependencyError::Evidence(delay_error))
            }),
            Ok(Views {
                static_imports: Ok(0),
                delay_imports: Err(PeDelayDependencyError::Names(name_error))
            }),
            Ok(Views {
                static_imports: Ok(0),
                delay_imports: Ok(Some(0))
            }),
            Ok(Views {
                static_imports: Ok(0),
                delay_imports: Ok(None)
            }),
        ]
    );
    assert_eq!(
        (output.total_requests, output.total_request_text_bytes),
        (1, 5)
    );
    assert_eq!(output.requests[0].candidate, Ok(Some(5)));
}

#[test]
fn complete_path_then_index_then_count_then_text_admission() {
    let mut input = batch(vec![
        source("app.exe", &["a.dll"], Some(&["a.dll"])),
        source("a.dll", &["a.dll"], None),
    ]);
    let mut caps = limits();
    caps.paths.max_paths = 1;
    assert_eq!(
        observe(&input, 9, caps),
        Err(Error::Paths(PathError::PathCountExceeded {
            count: 2,
            limit: 1
        }))
    );
    caps = limits();
    input.entries[1].path.normalized = "bad/../file".into();
    assert_eq!(
        observe(&input, 9, caps),
        Err(Error::Paths(PathError::Segment {
            index: 1,
            segment: 1,
            reason: AsciiSourcePathSegmentError::Parent
        }))
    );
    input.entries[1].path.normalized = "a.dll".into();
    caps.max_requests = 0;
    assert_eq!(
        observe(&input, 2, caps),
        Err(Error::ApplicationSourceIndexOutOfRange { index: 2, count: 2 })
    );
    caps.max_requests = 1;
    caps.max_request_text_bytes = 0;
    assert_eq!(
        observe(&input, 0, caps),
        Err(Error::RequestCountExceeded { count: 3, limit: 1 })
    );
    caps.max_requests = 3;
    caps.max_request_text_bytes = 8;
    assert_eq!(
        observe(&input, 0, caps),
        Err(Error::RequestTextExceeded {
            bytes: 15,
            limit: 8
        })
    );
    caps.max_request_text_bytes = 15;
    caps.max_basename_bytes = 5;
    assert_eq!(observe(&input, 0, caps).unwrap().total_requests, 3);
}

#[test]
fn invalid_tokens_are_charged_and_kept_as_per_request_errors() {
    let input = batch(vec![source(
        "app.exe",
        &["é", "", "a/b", "missing", "app.exe"],
        None,
    )]);
    let mut caps = limits();
    caps.max_request_text_bytes = 18;
    assert_eq!(
        observe(&input, 0, caps),
        Err(Error::RequestTextExceeded {
            bytes: 19,
            limit: 18
        })
    );
    caps.max_request_text_bytes = 19;
    let output = observe(&input, 0, caps).unwrap();
    assert_eq!(
        output
            .requests
            .iter()
            .map(|r| r.candidate)
            .collect::<Vec<_>>(),
        vec![
            Err(AsciiApplicationSourceCandidateError::Basename(
                PathError::NonAsciiByte {
                    index: 0,
                    offset: 0
                }
            )),
            Err(AsciiApplicationSourceCandidateError::Basename(
                PathError::EmptyPath { index: 0 }
            )),
            Err(AsciiApplicationSourceCandidateError::Basename(
                PathError::DepthExceeded {
                    index: 0,
                    depth: 2,
                    limit: 1
                }
            )),
            Ok(None),
            Ok(Some(0)),
        ]
    );
    caps.max_basename_bytes = 0;
    assert_eq!(
        observe(&input, 0, caps).unwrap().requests[0].candidate,
        Err(AsciiApplicationSourceCandidateError::Basename(
            PathError::PathBytesExceeded {
                index: 0,
                bytes: 2,
                limit: 0
            }
        ))
    );
}

#[test]
fn empty_success_and_reordered_source_positions_are_explicit() {
    let mut caps = limits();
    caps.max_requests = 0;
    caps.max_request_text_bytes = 0;
    caps.max_basename_bytes = 0;
    assert_eq!(
        observe(&batch(vec![]), 0, caps),
        Err(Error::ApplicationSourceIndexOutOfRange { index: 0, count: 0 })
    );
    assert!(
        observe(&batch(vec![source("app.exe", &[], Some(&[]))]), 0, caps)
            .unwrap()
            .requests
            .is_empty()
    );
    let input = batch(vec![
        source("A.DLL", &[], None),
        source("app.exe", &["a.dll"], None),
    ]);
    let output = observe(&input, 1, limits()).unwrap();
    assert_eq!(output.requests[0].source_index, 1);
    assert_eq!(output.requests[0].candidate, Ok(Some(0)));
}

mod closure {
    use super::{batch, limits, source};
    use ring3_core::{
        AsciiPeDependencyClosure, AsciiPeDependencyClosureError as Error,
        AsciiPeDependencyClosureLimits as Limits, AsciiPeModuleDependencyError,
        PeDependencyClosureMode::{StaticAndDelay, StaticOnly},
        PeDependencyRequestStep::{
            self, AlreadyReached, CandidateError, Discovered, ExcludedDelay, NoCandidate,
        },
        PeFingerprintError, observe_ascii_pe_module_dependencies as observe,
        walk_ascii_pe_dependency_closure as walk,
    };

    fn caps(reached: u64, examined: u64) -> Limits {
        Limits {
            observation: limits(),
            max_reached_sources: reached,
            max_examined_requests: examined,
        }
    }
    fn visits(value: &AsciiPeDependencyClosure) -> Vec<(usize, Option<usize>)> {
        value
            .visits
            .iter()
            .map(|v| (v.source_index, v.via_request_index))
            .collect()
    }
    fn steps(value: &AsciiPeDependencyClosure) -> Vec<(usize, PeDependencyRequestStep)> {
        value
            .examined_requests
            .iter()
            .map(|v| (v.request_index, v.step))
            .collect()
    }

    fn replace_ignored_metadata(input: &mut ring3_core::AsciiPeSourceModuleEvidenceBatch) {
        input.total_path_bytes = 0;
        input.total_content_bytes = 0;
        for entry in &mut input.entries {
            entry.path.index = usize::MAX;
            entry.path.key = "app/b.dll".into();
            entry.path.depth = 0;
            let module = entry.module.as_mut().unwrap();
            module.fingerprinted.byte_length = u64::MAX;
            module.fingerprinted.digest.fill(165);
            let header_error = ring3_core::PeHeaderError::InvalidDosSignature {
                offset: ring3_core::FileOffset::new(u64::MAX),
            };
            module.fingerprinted.evidence.prefix = Err(header_error);
            module.fingerprinted.evidence.optional = Err(header_error);
            module.fingerprinted.evidence.clr =
                Err(ring3_core::PeClrError::UnsupportedHeaderSize {
                    header_size: 0,
                    required: 72,
                });
            let imports = module.static_imports.as_mut().unwrap();
            imports.total_rows = 0;
            imports.total_text_bytes = 0;
            imports.lookups = Ok(vec![]);
            let delay = module.delay_imports.as_mut().unwrap();
            delay.total_rows = 0;
            delay.total_text_bytes = 0;
            delay.descriptors = Err(ring3_core::PeDelayImportError::MissingTerminator {
                descriptor_index: 73,
            });
            delay.lookups = Err(ring3_core::PeDelayImportLookupError::Lookup(
                ring3_core::PeImportLookupError::LookupTableUnavailable {
                    descriptor_index: 71,
                },
            ));
            module.exports = Err(ring3_core::PeExportEvidenceError::OutputRowsExceeded {
                rows: u64::MAX,
                limit: 0,
            });
            module.bound_imports =
                Err(ring3_core::PeBoundImportEvidenceError::OutputTextExceeded {
                    bytes: u64::MAX,
                    limit: 0,
                });
        }
    }

    #[test]
    fn ignored_metadata_preserves_observations_and_both_closure_modes() {
        use super::Kind::{Delay, Static};
        let input = batch(vec![
            source("other/a.dll", &["ignored.dll"], None),
            source("app/a.dll", &["main.exe"], None),
            source("app/main.exe", &["a.dll"], Some(&["b.dll"])),
            source("app/b.dll", &[], None),
        ]);
        let original = input.clone();
        let observations = observe(&input, 2, limits()).unwrap();
        assert_eq!(observations.total_requests, 4);
        assert_eq!(observations.total_request_text_bytes, 29);
        assert_eq!(observations.paths.total_path_bytes, 41);
        assert_eq!(
            observations
                .requests
                .iter()
                .map(|r| (
                    r.source_index,
                    r.kind,
                    r.descriptor_index,
                    r.dll_name.as_str(),
                    r.candidate,
                ))
                .collect::<Vec<_>>(),
            [
                (0, Static, 0, "ignored.dll", Ok(None)),
                (1, Static, 0, "main.exe", Ok(Some(2))),
                (2, Static, 0, "a.dll", Ok(Some(1))),
                (2, Delay, 0, "b.dll", Ok(Some(3))),
            ]
        );
        let mut changed = input.clone();
        replace_ignored_metadata(&mut changed);
        let before = changed.clone();
        let observed = observe(&changed, 2, limits()).unwrap();
        assert_eq!(observed, observations);
        assert_eq!(observe(&changed, 2, limits()), Ok(observed.clone()));
        let mut retained = Vec::new();
        for (mode, expected_visits, expected_steps) in [
            (
                StaticOnly,
                vec![(2, None), (1, Some(2))],
                vec![
                    (2, Discovered { visit_index: 1 }),
                    (3, ExcludedDelay),
                    (1, AlreadyReached { visit_index: 0 }),
                ],
            ),
            (
                StaticAndDelay,
                vec![(2, None), (1, Some(2)), (3, Some(3))],
                vec![
                    (2, Discovered { visit_index: 1 }),
                    (3, Discovered { visit_index: 2 }),
                    (1, AlreadyReached { visit_index: 0 }),
                ],
            ),
        ] {
            let expected = walk(&input, 2, mode, caps(3, 3)).unwrap();
            assert_eq!(expected.observations, observations);
            assert_eq!(visits(&expected), expected_visits);
            assert_eq!(steps(&expected), expected_steps);
            let actual = walk(&changed, 2, mode, caps(3, 3)).unwrap();
            assert_eq!(actual, expected);
            assert_eq!(walk(&changed, 2, mode, caps(3, 3)), Ok(actual.clone()));
            retained.push((actual, expected));
        }
        assert_eq!(input, original);
        assert_eq!(changed, before);
        changed.entries.clear();
        drop(changed);
        assert_eq!(observed, observations);
        for (actual, expected) in retained {
            assert_eq!(actual, expected);
        }

        assert_authoritative_fields_change_results(input);
    }

    fn assert_authoritative_fields_change_results(
        mut authoritative: ring3_core::AsciiPeSourceModuleEvidenceBatch,
    ) {
        authoritative.entries[2].path.normalized.clear();
        let path_error =
            AsciiPeModuleDependencyError::Paths(ring3_core::AsciiSourcePathError::EmptyPath {
                index: 2,
            });
        assert_eq!(observe(&authoritative, 2, limits()), Err(path_error));
        for mode in [StaticOnly, StaticAndDelay] {
            assert_eq!(
                walk(&authoritative, 2, mode, caps(3, 3)),
                Err(Error::Observation(path_error))
            );
        }
        authoritative.entries[2].path.normalized = "app/main.exe".into();
        authoritative.entries[2]
            .module
            .as_mut()
            .unwrap()
            .static_imports
            .as_mut()
            .unwrap()
            .descriptors
            .as_mut()
            .unwrap()[0]
            .dll_name = "missing.dll".into();
        assert_eq!(
            observe(&authoritative, 2, limits()).unwrap().requests[2].candidate,
            Ok(None)
        );
        for mode in [StaticOnly, StaticAndDelay] {
            assert_eq!(
                walk(&authoritative, 2, mode, caps(3, 3))
                    .unwrap()
                    .examined_requests[0]
                    .step,
                NoCandidate
            );
        }
    }

    #[test]
    fn breadth_first_provenance_uses_original_request_indices_and_owns_the_report() {
        let mut input = batch(vec![
            source("c.dll", &[], None),
            source("b.dll", &["c.dll"], None),
            source("app.exe", &["a.dll"], Some(&["b.dll"])),
            source("a.dll", &["c.dll"], None),
        ]);
        let before = input.clone();
        let observations = observe(&input, 2, limits()).unwrap();
        let output = walk(&input, 2, StaticAndDelay, caps(4, 4)).unwrap();
        assert_eq!(input, before);
        assert_eq!(
            walk(&input, 2, StaticAndDelay, caps(4, 4)),
            Ok(output.clone())
        );
        for entry in &mut input.entries {
            entry.path.normalized.replace_range(.., "overwritten");
            if let Ok(module) = &mut entry.module {
                for row in module
                    .static_imports
                    .as_mut()
                    .unwrap()
                    .descriptors
                    .as_mut()
                    .unwrap()
                {
                    row.dll_name.replace_range(.., "overwritten");
                }
                if let Some(table) = module
                    .delay_imports
                    .as_mut()
                    .unwrap()
                    .names
                    .as_mut()
                    .unwrap()
                {
                    for row in &mut table.imports {
                        row.dll_name.replace_range(.., "overwritten");
                    }
                }
            }
        }
        drop(input);
        assert_eq!(output.observations, observations);
        assert_eq!(output.mode, StaticAndDelay);
        assert_eq!(
            visits(&output),
            [(2, None), (3, Some(1)), (1, Some(2)), (0, Some(3))]
        );
        assert_eq!(
            steps(&output),
            [
                (1, Discovered { visit_index: 1 }),
                (2, Discovered { visit_index: 2 }),
                (3, Discovered { visit_index: 3 }),
                (0, AlreadyReached { visit_index: 3 })
            ]
        );
    }

    #[test]
    fn delay_modes_keep_excluded_occurrences_and_later_static_reachability() {
        let input = batch(vec![
            source("app.exe", &["a.dll"], Some(&["b.dll"])),
            source("a.dll", &["b.dll"], None),
            source("b.dll", &[], None),
        ]);
        let static_only = walk(&input, 0, StaticOnly, caps(3, 3)).unwrap();
        let potential = walk(&input, 0, StaticAndDelay, caps(3, 3)).unwrap();
        assert_eq!(static_only.mode, StaticOnly);
        assert_eq!(static_only.observations, potential.observations);
        assert_eq!(
            visits(&static_only),
            [(0, None), (1, Some(0)), (2, Some(2))]
        );
        assert_eq!(visits(&potential), [(0, None), (1, Some(0)), (2, Some(1))]);
        assert_eq!(
            steps(&static_only),
            [
                (0, Discovered { visit_index: 1 }),
                (1, ExcludedDelay),
                (2, Discovered { visit_index: 2 })
            ]
        );
        assert_eq!(
            steps(&potential),
            [
                (0, Discovered { visit_index: 1 }),
                (1, Discovered { visit_index: 2 }),
                (2, AlreadyReached { visit_index: 2 })
            ]
        );
        let only_delay = batch(vec![source("app.exe", &[], Some(&["missing.dll"]))]);
        assert_eq!(
            walk(&only_delay, 0, StaticOnly, caps(1, 0)),
            Err(Error::ExaminedRequestsExceeded {
                source_index: 0,
                request_index: 0,
                count: 1,
                limit: 0
            })
        );
    }

    #[test]
    fn cycles_duplicates_and_equal_module_aliases_preserve_each_occurrence() {
        let a = source("a.dll", &["app.exe", "shared.dll"], None);
        let mut alias = a.clone();
        alias.path.normalized = "alias.dll".into();
        let input = batch(vec![
            source("app.exe", &["app.exe", "a.dll", "A.DLL", "alias.dll"], None),
            a,
            alias,
            source("shared.dll", &[], None),
        ]);
        assert_eq!(input.entries[1].module, input.entries[2].module);
        let output = walk(&input, 0, StaticOnly, caps(4, 8)).unwrap();
        assert_eq!(
            visits(&output),
            [(0, None), (1, Some(1)), (2, Some(3)), (3, Some(5))]
        );
        assert_eq!(
            steps(&output),
            [
                (0, AlreadyReached { visit_index: 0 }),
                (1, Discovered { visit_index: 1 }),
                (2, AlreadyReached { visit_index: 1 }),
                (3, Discovered { visit_index: 2 }),
                (4, AlreadyReached { visit_index: 0 }),
                (5, Discovered { visit_index: 3 }),
                (6, AlreadyReached { visit_index: 0 }),
                (7, AlreadyReached { visit_index: 3 })
            ]
        );
        assert_eq!(
            walk(&input, 0, StaticOnly, caps(4, 7)),
            Err(Error::ExaminedRequestsExceeded {
                source_index: 2,
                request_index: 7,
                count: 8,
                limit: 7
            })
        );
    }

    #[test]
    fn inventory_preflight_root_and_edge_admission_have_explicit_precedence() {
        let input = batch(vec![
            source("app.exe", &["a.dll"], None),
            source("a.dll", &["b.dll"], None),
            source("b.dll", &[], None),
        ]);
        let mut limited = caps(0, 0);
        limited.observation.max_requests = 1;
        assert_eq!(
            walk(&input, 0, StaticOnly, limited),
            Err(Error::Observation(
                AsciiPeModuleDependencyError::RequestCountExceeded { count: 2, limit: 1 }
            ))
        );
        assert_eq!(
            walk(&input, 0, StaticOnly, caps(0, 0)),
            Err(Error::ReachedSourcesExceeded {
                source_index: 0,
                via_request_index: None,
                count: 1,
                limit: 0
            })
        );
        assert_eq!(
            walk(&input, 0, StaticOnly, caps(1, 0)),
            Err(Error::ExaminedRequestsExceeded {
                source_index: 0,
                request_index: 0,
                count: 1,
                limit: 0
            })
        );
        assert_eq!(
            walk(&input, 0, StaticOnly, caps(1, 1)),
            Err(Error::ReachedSourcesExceeded {
                source_index: 1,
                via_request_index: Some(0),
                count: 2,
                limit: 1
            })
        );
        assert_eq!(
            walk(&input, 0, StaticOnly, caps(2, 1)),
            Err(Error::ExaminedRequestsExceeded {
                source_index: 1,
                request_index: 1,
                count: 2,
                limit: 1
            })
        );
        assert_eq!(
            walk(&input, 0, StaticOnly, caps(2, 2)),
            Err(Error::ReachedSourcesExceeded {
                source_index: 2,
                via_request_index: Some(1),
                count: 3,
                limit: 2
            })
        );
        assert_eq!(
            visits(&walk(&input, 0, StaticOnly, caps(3, 2)).unwrap()),
            [(0, None), (1, Some(0)), (2, Some(1))]
        );
        let unrelated = batch(vec![
            source("app.exe", &[], None),
            source("unused.dll", &["missing.dll"], None),
        ]);
        let mut limited = caps(0, 0);
        limited.observation.max_request_text_bytes = 0;
        assert_eq!(
            walk(&unrelated, 0, StaticOnly, limited),
            Err(Error::Observation(
                AsciiPeModuleDependencyError::RequestTextExceeded {
                    bytes: 11,
                    limit: 0
                }
            ))
        );
        let output = walk(&unrelated, 0, StaticOnly, caps(1, 0)).unwrap();
        assert_eq!(visits(&output), [(0, None)]);
        assert!(output.examined_requests.is_empty());
        assert_eq!(output.observations.requests.len(), 1);
    }

    #[test]
    fn terminal_frontiers_retain_errors_and_exclusion_precedes_candidate_classification() {
        let error = PeFingerprintError::InputTooLarge {
            length: 2,
            limit: 1,
        };
        let mut failed = source("broken.dll", &[], None);
        failed.module = Err(error);
        let input = batch(vec![
            source(
                "app.exe",
                &["missing.dll", "é.dll", "broken.dll"],
                Some(&["é.dll"]),
            ),
            failed,
        ]);
        let output = walk(&input, 0, StaticOnly, caps(2, 4)).unwrap();
        assert_eq!(visits(&output), [(0, None), (1, Some(2))]);
        assert_eq!(
            steps(&output),
            [
                (0, NoCandidate),
                (1, CandidateError),
                (2, Discovered { visit_index: 1 }),
                (3, ExcludedDelay)
            ]
        );
        assert_eq!(output.observations.sources[1], Err(error));
        assert!(output.observations.requests[1].candidate.is_err());
        assert!(output.observations.requests[3].candidate.is_err());
        assert_eq!(
            steps(&walk(&input, 0, StaticAndDelay, caps(2, 4)).unwrap())[3],
            (3, CandidateError)
        );
    }
}
