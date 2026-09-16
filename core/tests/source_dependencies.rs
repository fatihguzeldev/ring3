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
