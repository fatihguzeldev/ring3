use super::{linked_forwarder_step, linked_terminal_step, route};
use ring3_core::{
    FileOffset, PeExportAddressEntry, PeExportBatchError, PeExportEvidence, PeExportEvidenceLimits,
    PeExportEvidenceLookupLimits, PeExportName, PeExportQuery, PeExportSelection, PeExportTarget,
    PeForwarderEvidenceWalkBatch, PeForwarderEvidenceWalkError, PeForwarderQuery,
    PeForwarderRequestError, PeForwarderStep, PeForwarderWalk, PeForwarderWalkBatchError,
    PeForwarderWalkBatchLimits, PeForwarderWalkBatchMetric, PeForwarderWalkError,
    PeForwarderWalkLimits, PeKind, PeOwnedExportTarget, RelativeVirtualAddress, inspect_pe_exports,
    parse_pe_header_prefix, walk_pe_export_evidence_forwarders,
    walk_pe_export_evidence_forwarders_batch, walk_pe_export_forwarders,
};

fn expected() -> PeForwarderWalk<'static> {
    PeForwarderWalk {
        selection_rows: 2,
        text_bytes: 42,
        steps: vec![
            linked_forwarder_step(false),
            PeForwarderStep {
                source_index: 1,
                query: PeForwarderQuery::Name("ring3_target".into()),
                selection: PeExportSelection::Selected {
                    address: PeExportAddressEntry {
                        table_index: 0,
                        ordinal: 32768,
                        entry_rva: RelativeVirtualAddress::new(8248),
                        entry_file_offset: FileOffset::new(1592),
                        target: PeExportTarget::Rva(RelativeVirtualAddress::new(4096)),
                    },
                    name: Some(PeExportName {
                        table_index: 0,
                        name_pointer_rva: RelativeVirtualAddress::new(8252),
                        name_pointer_file_offset: FileOffset::new(1596),
                        ordinal_entry_rva: RelativeVirtualAddress::new(8256),
                        ordinal_entry_file_offset: FileOffset::new(1600),
                        address_index: 0,
                        name_rva: RelativeVirtualAddress::new(8258),
                        name_file_offset: FileOffset::new(1602),
                        name: "ring3_target",
                    }),
                },
                forwarder: None,
            },
        ],
    }
}

fn limits() -> PeForwarderWalkLimits {
    PeForwarderWalkLimits {
        max_sources: 2,
        max_routes: 1,
        max_hops: 2,
        max_selection_rows: 2,
        max_text_bytes: 42,
    }
}

fn query_limits() -> PeExportEvidenceLookupLimits {
    PeExportEvidenceLookupLimits {
        max_table_rows: 5,
        max_table_text_bytes: 59,
    }
}

fn refusals() -> [(PeForwarderWalkLimits, PeForwarderWalkError<'static>); 2] {
    [
        (
            PeForwarderWalkLimits {
                max_selection_rows: 1,
                ..limits()
            },
            PeForwarderWalkError::SelectionRows {
                hop: 1,
                source_index: 1,
                used: 1,
                cause: PeExportBatchError::SelectionRowsExceeded {
                    index: 0,
                    total: 1,
                    limit: 0,
                },
            },
        ),
        (
            PeForwarderWalkLimits {
                max_text_bytes: 41,
                ..limits()
            },
            PeForwarderWalkError::Decode {
                hop: 0,
                source_index: 0,
                used_text_bytes: 18,
                cause: PeForwarderRequestError::LengthLimitExceeded {
                    length: 24,
                    limit: 23,
                },
            },
        ),
    ]
}

fn missing_route() -> PeForwarderWalkError<'static> {
    PeForwarderWalkError::MissingRoute {
        hop: 0,
        source_index: 0,
        request: linked_forwarder_step(false).forwarder.unwrap().request,
    }
}

fn pointers(walk: &PeForwarderWalk<'_>, forwarder: *const u8, provider: *const u8) {
    let request = walk.steps[0].forwarder.unwrap().request;
    assert!(std::ptr::eq(request.module.as_ptr(), forwarder));
    let PeExportSelection::Selected { address, .. } = walk.steps[0].selection else {
        panic!()
    };
    let PeExportTarget::Forwarder { text, .. } = address.target else {
        panic!()
    };
    assert!(std::ptr::eq(text.as_ptr(), forwarder));
    let PeExportSelection::Selected {
        name: Some(name), ..
    } = walk.steps[1].selection
    else {
        panic!()
    };
    assert!(std::ptr::eq(name.name.as_ptr(), provider));
}

fn check_borrowed(forwarder: &[u8], provider: &[u8], wrong_provider: &[u8]) {
    let sources = [forwarder, provider];
    let routes = [route(0, "OtherModule", 1)];
    let run = |routes: &[_], cap| {
        walk_pe_export_forwarders(&sources, routes, 0, PeExportQuery::Name("by_name"), cap)
    };
    let walk = run(&routes, limits()).unwrap();
    assert_eq!(walk, expected());
    assert_eq!(run(&routes, limits()), Ok(expected()));
    pointers(&walk, forwarder[1639..].as_ptr(), provider[1602..].as_ptr());
    assert_eq!(run(&[], limits()), Err(missing_route()));
    for (cap, error) in refusals() {
        assert_eq!(run(&routes, cap), Err(error));
    }
    assert_eq!(
        walk_pe_export_forwarders(
            &[forwarder, wrong_provider],
            &routes,
            0,
            PeExportQuery::Name("by_name"),
            limits()
        ),
        Ok(PeForwarderWalk {
            selection_rows: 1,
            text_bytes: 42,
            steps: vec![linked_forwarder_step(false), linked_terminal_step(false)],
        })
    );
}

fn observe(mut bytes: Vec<u8>, rows: u64, text: u64) -> PeExportEvidence {
    let evidence = inspect_pe_exports(
        &bytes,
        PeExportEvidenceLimits {
            max_input_bytes: 2048,
            max_output_rows: rows,
            max_output_text_bytes: text,
        },
    )
    .unwrap();
    assert_eq!(
        (evidence.total_rows, evidence.total_text_bytes),
        (rows, text)
    );
    bytes.fill(0xee);
    drop(bytes);
    evidence
}

fn retained_pointers(walk: &PeForwarderWalk<'_>, evidence: &[PeExportEvidence; 2]) {
    let forwarder = &evidence[0]
        .names
        .as_ref()
        .unwrap()
        .as_ref()
        .unwrap()
        .addresses
        .entries[0]
        .target;
    let PeOwnedExportTarget::Forwarder { text, .. } = forwarder else {
        panic!()
    };
    let provider = &evidence[1]
        .names
        .as_ref()
        .unwrap()
        .as_ref()
        .unwrap()
        .entries[0]
        .name;
    pointers(walk, text.as_ptr(), provider.as_ptr());
}

fn check_owned(evidence: &[PeExportEvidence; 2]) {
    let sources = [&evidence[0], &evidence[1]];
    let routes = [route(0, "OtherModule", 1)];
    let run = |routes: &[_], cap| {
        walk_pe_export_evidence_forwarders(
            &sources,
            routes,
            0,
            PeExportQuery::Name("by_name"),
            query_limits(),
            cap,
        )
    };
    let walk = run(&routes, limits()).unwrap();
    assert_eq!(walk, expected());
    assert_eq!(run(&routes, limits()), Ok(expected()));
    retained_pointers(&walk, evidence);
    assert_eq!(
        run(&[], limits()),
        Err(PeForwarderEvidenceWalkError::Walk(missing_route()))
    );
    for (cap, error) in refusals() {
        assert_eq!(
            run(&routes, cap),
            Err(PeForwarderEvidenceWalkError::Walk(error))
        );
    }
}

fn check_batch(evidence: &[PeExportEvidence; 2]) {
    let cap = PeForwarderWalkBatchLimits {
        max_queries: 2,
        max_successful_steps: 4,
        max_successful_selection_rows: 4,
        max_successful_text_bytes: 84,
    };
    let run = |cap| {
        walk_pe_export_evidence_forwarders_batch(
            &[&evidence[0], &evidence[1]],
            &[route(0, "OtherModule", 1)],
            0,
            &[
                PeExportQuery::Name("by_name"),
                PeExportQuery::Name("by_name"),
            ],
            query_limits(),
            limits(),
            cap,
        )
    };
    let want = PeForwarderEvidenceWalkBatch {
        successful_steps: 4,
        successful_selection_rows: 4,
        successful_text_bytes: 84,
        walks: vec![Ok(expected()), Ok(expected())],
    };
    let batch = run(cap).unwrap();
    assert_eq!(batch, want);
    for walk in &batch.walks {
        retained_pointers(walk.as_ref().unwrap(), evidence);
    }
    assert_eq!(run(cap), Ok(want));
    assert_eq!(
        run(PeForwarderWalkBatchLimits {
            max_queries: 1,
            ..cap
        }),
        Err(PeForwarderWalkBatchError::QueryCountExceeded { count: 2, limit: 1 })
    );
    for (metric, limited, used, amount, limit) in [
        (
            PeForwarderWalkBatchMetric::SuccessfulSteps,
            PeForwarderWalkBatchLimits {
                max_successful_steps: 3,
                ..cap
            },
            2,
            2,
            3,
        ),
        (
            PeForwarderWalkBatchMetric::SuccessfulSelectionRows,
            PeForwarderWalkBatchLimits {
                max_successful_selection_rows: 3,
                ..cap
            },
            2,
            2,
            3,
        ),
        (
            PeForwarderWalkBatchMetric::SuccessfulTextBytes,
            PeForwarderWalkBatchLimits {
                max_successful_text_bytes: 83,
                ..cap
            },
            42,
            42,
            83,
        ),
    ] {
        assert_eq!(
            run(limited),
            Err(PeForwarderWalkBatchError::OutputLimitExceeded {
                metric,
                index: 1,
                used,
                amount,
                limit
            })
        );
    }
}

fn check_fixture(variables: [&str; 3], kind: PeKind) {
    let paths = variables.map(|key| {
        std::path::PathBuf::from(
            std::env::var_os(key)
                .expect("all explicit compiled named forwarder paths are required"),
        )
    });
    let bytes = paths.each_ref().map(|path| std::fs::read(path).unwrap());
    for bytes in &bytes {
        assert_eq!(bytes.len(), 2048);
        assert_eq!(parse_pe_header_prefix(bytes).unwrap().kind, kind);
    }
    let before = bytes.clone();
    check_borrowed(&bytes[0], &bytes[1], &bytes[2]);
    assert_eq!(bytes, before);
    let [forwarder, provider, _wrong_provider] = bytes;
    let evidence = [observe(forwarder, 8, 101), observe(provider, 3, 12)];
    check_owned(&evidence);
    check_batch(&evidence);
    for (path, original) in paths.iter().zip(before) {
        assert_eq!(std::fs::read(path).unwrap(), original);
    }
}

#[test]
#[ignore = "requires explicit compiled PE32 forwarder, named and ordinal providers"]
fn generated_pe32_named_forwarder_selects_terminal() {
    check_fixture(
        [
            "RING3_EXPORT_FORWARD_PE32_FIXTURE",
            "RING3_EXPORT_FORWARD_PE32_NAMED_PROVIDER",
            "RING3_EXPORT_PE32_ORDINAL_DLL",
        ],
        PeKind::Pe32,
    );
}

#[test]
#[ignore = "requires explicit compiled PE32+ forwarder, named and ordinal providers"]
fn generated_pe32plus_named_forwarder_selects_terminal() {
    check_fixture(
        [
            "RING3_EXPORT_FORWARD_PE32PLUS_FIXTURE",
            "RING3_EXPORT_FORWARD_PE32PLUS_NAMED_PROVIDER",
            "RING3_EXPORT_PE32PLUS_ORDINAL_DLL",
        ],
        PeKind::Pe32Plus,
    );
}
