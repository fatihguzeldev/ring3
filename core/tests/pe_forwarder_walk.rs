use ring3_core::{
    FileOffset, PeExportBatchError, PeExportQuery, PeExportSelection, PeExportTarget,
    PeForwarderQuery, PeForwarderRequestError, PeForwarderRoute, PeForwarderTextContext,
    PeForwarderWalkError, PeForwarderWalkLimits, RelativeVirtualAddress, walk_pe_export_forwarders,
};

fn limits() -> PeForwarderWalkLimits {
    PeForwarderWalkLimits {
        max_sources: 4,
        max_routes: 4,
        max_hops: 8,
        max_selection_rows: 8,
        max_text_bytes: 256,
    }
}

#[test]
fn source_admission_precedes_root_and_provider_errors() {
    assert_eq!(
        walk_pe_export_forwarders(
            &[b""],
            &[],
            u32::MAX,
            PeExportQuery::Ordinal(1),
            PeForwarderWalkLimits {
                max_sources: 0,
                ..limits()
            },
        ),
        Err(PeForwarderWalkError::SourceCountExceeded { count: 1, limit: 0 })
    );
}

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn fixture(plus: bool, forwarder: Option<&str>) -> Vec<u8> {
    let mut bytes = vec![0; 4096];
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 60, 128);
    bytes[128..132].copy_from_slice(b"PE\0\0");
    bytes[134..136].copy_from_slice(&1_u16.to_le_bytes());
    let fixed = if plus { 112_u16 } else { 96 };
    bytes[148..150].copy_from_slice(&(fixed + 8).to_le_bytes());
    bytes[152..154].copy_from_slice(&if plus { 0x20b_u16 } else { 0x10b }.to_le_bytes());
    put32(&mut bytes, 212, 512);
    put32(&mut bytes, 152 + usize::from(fixed) - 4, 1);
    let directory = 152 + usize::from(fixed);
    put32(&mut bytes, directory, 0x1000);
    put32(&mut bytes, directory + 4, 0x200);
    let section = directory + 8;
    for (offset, value) in [(8, 3584), (12, 0x1000), (16, 3584), (20, 512)] {
        put32(&mut bytes, section + offset, value);
    }
    for (offset, value) in [
        (16, 7),
        (20, 2),
        (24, 1),
        (28, 0x1200),
        (32, 0x1210),
        (36, 0x1220),
    ] {
        put32(&mut bytes, 512 + offset, value);
    }
    put32(
        &mut bytes,
        1024,
        if forwarder.is_some() {
            0x1100
        } else {
            0x0040_0000
        },
    );
    put32(&mut bytes, 1028, 0x0050_0000);
    put32(&mut bytes, 1040, 0x1230);
    bytes[1072..1078].copy_from_slice(b"entry\0");
    if let Some(text) = forwarder {
        bytes[768..768 + text.len()].copy_from_slice(text.as_bytes());
    }
    bytes
}

fn route(
    source_index: u32,
    requested_module: &str,
    destination_index: u32,
) -> PeForwarderRoute<'_> {
    PeForwarderRoute {
        source_index,
        requested_module,
        destination_index,
    }
}

#[test]
fn both_widths_follow_exact_routes_and_retain_raw_coordinates() {
    for plus in [false, true] {
        let first = fixture(plus, Some("Next.#0007"));
        let second = fixture(!plus, None);
        let result = walk_pe_export_forwarders(
            &[&first, &second],
            &[route(0, "Next", 1)],
            0,
            PeExportQuery::Name("entry"),
            PeForwarderWalkLimits {
                max_sources: 2,
                max_routes: 1,
                max_hops: 2,
                max_selection_rows: 2,
                max_text_bytes: 19,
            },
        )
        .unwrap();
        assert_eq!((result.selection_rows, result.text_bytes), (2, 19));
        assert_eq!(result.steps.len(), 2);
        assert_eq!(
            result.steps[0].query,
            PeForwarderQuery::Name("entry".into())
        );
        assert_eq!(result.steps[1].query, PeForwarderQuery::Ordinal(7));
        let hop = result.steps[0].forwarder.unwrap();
        assert_eq!((hop.request.module, hop.destination_index), ("Next", 1));
        assert!(result.steps[1].forwarder.is_none());
        let PeExportSelection::Selected {
            address,
            name: Some(name),
        } = result.steps[0].selection
        else {
            panic!()
        };
        assert_eq!((address.table_index, address.ordinal), (0, 7));
        assert_eq!(address.entry_file_offset, FileOffset::new(1024));
        assert_eq!(name.name_file_offset, FileOffset::new(1072));
        assert!(std::ptr::eq(name.name.as_ptr(), first[1072..].as_ptr()));
        let PeExportSelection::Selected {
            address,
            name: None,
        } = result.steps[1].selection
        else {
            panic!()
        };
        assert_eq!(
            address.target,
            PeExportTarget::Rva(RelativeVirtualAddress::new(0x0040_0000))
        );
    }
}

#[test]
fn revisiting_a_source_allows_another_entry_but_refuses_name_ordinal_alias_cycles() {
    for plus in [false, true] {
        let first = fixture(plus, Some("Next.#7"));
        let second = fixture(!plus, Some("Back.#8"));
        let routes = [route(0, "Next", 1), route(1, "Back", 0)];
        let result = walk_pe_export_forwarders(
            &[&first, &second],
            &routes,
            0,
            PeExportQuery::Name("entry"),
            limits(),
        )
        .unwrap();
        assert_eq!(
            result
                .steps
                .iter()
                .map(|step| step.source_index)
                .collect::<Vec<_>>(),
            [0, 1, 0]
        );
        let PeExportSelection::Selected { address, .. } = result.steps[2].selection else {
            panic!()
        };
        assert_eq!((address.table_index, address.ordinal), (1, 8));
        let second = fixture(!plus, Some("Back.#7"));
        assert_eq!(
            walk_pe_export_forwarders(
                &[&first, &second],
                &routes,
                0,
                PeExportQuery::Name("entry"),
                limits()
            ),
            Err(PeForwarderWalkError::Cycle {
                first_hop: 0,
                hop: 2,
                source_index: 0,
                table_index: 0
            })
        );
    }
}

#[test]
fn identical_bytes_keep_positional_identity_and_routes_use_source_context() {
    let repeated = fixture(false, Some("Next.#7"));
    let terminal = fixture(true, None);
    let routes = [route(0, "Next", 1), route(1, "Next", 2)];
    let result = walk_pe_export_forwarders(
        &[&repeated, &repeated, &terminal],
        &routes,
        0,
        PeExportQuery::Ordinal(7),
        limits(),
    )
    .unwrap();
    assert_eq!(
        result
            .steps
            .iter()
            .map(|step| step.source_index)
            .collect::<Vec<_>>(),
        [0, 1, 2]
    );
    assert_eq!((result.selection_rows, result.text_bytes), (3, 22));
    for requested in ["next", "Next.dll"] {
        let error = walk_pe_export_forwarders(
            &[&repeated, &terminal],
            &[route(0, requested, 1)],
            0,
            PeExportQuery::Ordinal(7),
            limits(),
        )
        .unwrap_err();
        let PeForwarderWalkError::MissingRoute {
            hop,
            source_index,
            request,
        } = error
        else {
            panic!()
        };
        assert_eq!((hop, source_index, request.module), (0, 0, "Next"));
        assert!(std::ptr::eq(
            request.module.as_ptr(),
            repeated[768..].as_ptr()
        ));
    }
}

#[test]
fn each_invocation_rebinds_positions_without_stale_provider_results() {
    for plus in [false, true] {
        let first = fixture(plus, Some("Next.entry"));
        let terminal = fixture(!plus, None);
        let routes = [route(0, "Next", 1)];
        for source in [terminal.as_slice(), b"", terminal.as_slice()] {
            let result = walk_pe_export_forwarders(
                &[&first, source],
                &routes,
                0,
                PeExportQuery::Ordinal(7),
                limits(),
            );
            if source.is_empty() {
                assert!(matches!(
                    result,
                    Err(PeForwarderWalkError::Provider {
                        hop: 1,
                        source_index: 1,
                        ..
                    })
                ));
            } else {
                assert_eq!(result.unwrap().steps.len(), 2);
            }
        }
    }
}

#[test]
fn unused_routes_are_admitted_in_count_text_source_destination_duplicate_order() {
    let sources: &[&[u8]] = &[b""];
    let invalid = [route(u32::MAX, "x", u32::MAX)];
    let call = |routes: &[PeForwarderRoute<'_>], cap| {
        walk_pe_export_forwarders(sources, routes, u32::MAX, PeExportQuery::Name("root"), cap)
    };
    assert_eq!(
        call(
            &invalid,
            PeForwarderWalkLimits {
                max_routes: 0,
                max_text_bytes: 0,
                ..limits()
            }
        ),
        Err(PeForwarderWalkError::RouteCountExceeded { count: 1, limit: 0 })
    );
    assert_eq!(
        call(
            &invalid,
            PeForwarderWalkLimits {
                max_text_bytes: 0,
                ..limits()
            }
        ),
        Err(PeForwarderWalkError::TextBytesExceeded {
            context: PeForwarderTextContext::RouteToken { index: 0 },
            total: 1,
            limit: 0
        })
    );
    assert_eq!(
        call(
            &invalid,
            PeForwarderWalkLimits {
                max_text_bytes: 4,
                ..limits()
            }
        ),
        Err(PeForwarderWalkError::TextBytesExceeded {
            context: PeForwarderTextContext::RootName,
            total: 5,
            limit: 4
        })
    );
    assert_eq!(
        call(&invalid, limits()),
        Err(PeForwarderWalkError::RouteSourceOutOfRange {
            index: 0,
            source_index: u32::MAX,
            source_count: 1
        })
    );
    assert_eq!(
        call(&[route(0, "x", 1)], limits()),
        Err(PeForwarderWalkError::RouteDestinationOutOfRange {
            index: 0,
            destination_index: 1,
            source_count: 1
        })
    );
    assert_eq!(
        call(&[route(0, "x", 0), route(0, "x", 1)], limits()),
        Err(PeForwarderWalkError::RouteDestinationOutOfRange {
            index: 1,
            destination_index: 1,
            source_count: 1
        })
    );
    assert_eq!(
        call(&[route(0, "x", 0), route(0, "x", 0)], limits()),
        Err(PeForwarderWalkError::DuplicateRoute {
            first: 0,
            index: 1,
            source_index: 0,
            requested_module: "x".into()
        })
    );
    assert_eq!(
        call(&[], limits()),
        Err(PeForwarderWalkError::RootSourceOutOfRange {
            source_index: u32::MAX,
            source_count: 1
        })
    );
}

#[test]
fn hop_rows_cycle_and_forwarder_text_refusals_keep_their_priority() {
    let bytes = fixture(false, Some("Self.#7"));
    let routes = [route(0, "Self", 0)];
    let call =
        |cap| walk_pe_export_forwarders(&[&bytes], &routes, 0, PeExportQuery::Ordinal(7), cap);
    assert_eq!(
        call(PeForwarderWalkLimits {
            max_hops: 0,
            max_selection_rows: 0,
            ..limits()
        }),
        Err(PeForwarderWalkError::HopLimitExceeded { hop: 0, limit: 0 })
    );
    assert_eq!(
        call(PeForwarderWalkLimits {
            max_hops: 1,
            max_selection_rows: 1,
            ..limits()
        }),
        Err(PeForwarderWalkError::HopLimitExceeded { hop: 1, limit: 1 })
    );
    assert_eq!(
        call(PeForwarderWalkLimits {
            max_selection_rows: 1,
            ..limits()
        }),
        Err(PeForwarderWalkError::SelectionRows {
            hop: 1,
            source_index: 0,
            used: 1,
            cause: PeExportBatchError::SelectionRowsExceeded {
                index: 0,
                total: 1,
                limit: 0
            }
        })
    );
    assert_eq!(
        call(PeForwarderWalkLimits {
            max_selection_rows: 2,
            max_text_bytes: 11,
            ..limits()
        }),
        Err(PeForwarderWalkError::Cycle {
            first_hop: 0,
            hop: 1,
            source_index: 0,
            table_index: 0
        })
    );
    assert_eq!(
        call(PeForwarderWalkLimits {
            max_text_bytes: 10,
            ..limits()
        }),
        Err(PeForwarderWalkError::Decode {
            hop: 0,
            source_index: 0,
            used_text_bytes: 4,
            cause: PeForwarderRequestError::LengthLimitExceeded {
                length: 7,
                limit: 6
            }
        })
    );
    let malformed = fixture(true, Some("Self.#x"));
    assert_eq!(
        walk_pe_export_forwarders(&[&malformed], &[], 0, PeExportQuery::Ordinal(7), limits()),
        Err(PeForwarderWalkError::Decode {
            hop: 0,
            source_index: 0,
            used_text_bytes: 0,
            cause: PeForwarderRequestError::InvalidOrdinalDigit {
                offset: 6,
                byte: b'x'
            }
        })
    );
}

#[test]
fn terminal_selections_preserve_ambiguity_empty_absence_and_wide_ordinals() {
    for plus in [false, true] {
        let mut bytes = fixture(plus, None);
        for (query, expected) in [
            (
                PeExportQuery::Name("absent"),
                PeExportSelection::NameNotFound,
            ),
            (
                PeExportQuery::Ordinal(6),
                PeExportSelection::OrdinalBeforeBase {
                    ordinal: 6,
                    base: 7,
                },
            ),
            (
                PeExportQuery::Ordinal(9),
                PeExportSelection::OrdinalOutOfRange {
                    ordinal: 9,
                    base: 7,
                    address_count: 2,
                },
            ),
        ] {
            let result = walk_pe_export_forwarders(&[&bytes], &[], 0, query, limits()).unwrap();
            assert_eq!(result.selection_rows, 0);
            assert_eq!(result.steps[0].selection, expected);
        }
        put32(&mut bytes, 1024, 0);
        let empty =
            walk_pe_export_forwarders(&[&bytes], &[], 0, PeExportQuery::Ordinal(7), limits())
                .unwrap();
        assert!(
            matches!(empty.steps[0].selection, PeExportSelection::Selected { address, .. } if address.target == PeExportTarget::Empty)
        );
        put32(&mut bytes, 536, 2);
        put32(&mut bytes, 1044, 0x1230);
        let ambiguous =
            walk_pe_export_forwarders(&[&bytes], &[], 0, PeExportQuery::Name("entry"), limits())
                .unwrap();
        assert_eq!(ambiguous.selection_rows, 2);
        let PeExportSelection::AmbiguousName { matches } = &ambiguous.steps[0].selection else {
            panic!()
        };
        assert_eq!((matches[0].table_index, matches[1].table_index), (0, 1));
        put32(&mut bytes, 528, u32::MAX);
        put32(&mut bytes, 532, 1);
        let wide = walk_pe_export_forwarders(
            &[&bytes],
            &[],
            0,
            PeExportQuery::Ordinal(u32::MAX),
            limits(),
        )
        .unwrap();
        assert!(
            matches!(wide.steps[0].selection, PeExportSelection::Selected { address, .. } if address.ordinal == u32::MAX)
        );
        let slot = if plus { 264 } else { 248 };
        put32(&mut bytes, slot, 0);
        put32(&mut bytes, slot + 4, 0);
        let absent =
            walk_pe_export_forwarders(&[&bytes], &[], 0, PeExportQuery::Ordinal(7), limits())
                .unwrap();
        assert_eq!(
            absent.steps[0].selection,
            PeExportSelection::DirectoryAbsent
        );
    }
}

#[test]
fn results_outlive_query_route_storage_and_keep_image_borrows() {
    let first = fixture(false, Some("Next.entry"));
    let second = fixture(true, None);
    let before = (first.clone(), second.clone());
    let result = {
        let module = String::from("Next");
        let name = String::from("entry");
        walk_pe_export_forwarders(
            &[&first, &second],
            &[route(0, &module, 1)],
            0,
            PeExportQuery::Name(&name),
            limits(),
        )
        .unwrap()
    };
    assert_eq!(
        result.steps[0].query,
        PeForwarderQuery::Name("entry".into())
    );
    assert_eq!(
        result.steps[1].query,
        PeForwarderQuery::Name("entry".into())
    );
    let request = result.steps[0].forwarder.unwrap().request;
    assert!(std::ptr::eq(request.module.as_ptr(), first[768..].as_ptr()));
    let ring3_core::PeForwarderSymbol::Name(name) = request.symbol else {
        panic!()
    };
    assert!(std::ptr::eq(name.as_ptr(), first[773..].as_ptr()));
    let PeExportSelection::Selected {
        name: Some(name), ..
    } = result.steps[1].selection
    else {
        panic!()
    };
    assert!(std::ptr::eq(name.name.as_ptr(), second[1072..].as_ptr()));
    assert_eq!((first, second), before);
}

#[test]
fn long_chains_are_iterative_and_charge_exact_aggregate_limits() {
    let forwarder = fixture(false, Some("Next.#7"));
    let terminal = fixture(true, None);
    let mut sources = vec![forwarder.as_slice(); 255];
    sources.push(&terminal);
    let routes: Vec<_> = (0..255)
        .map(|index| route(index, "Next", index + 1))
        .collect();
    let cap = PeForwarderWalkLimits {
        max_sources: 256,
        max_routes: 255,
        max_hops: 256,
        max_selection_rows: 256,
        max_text_bytes: 2805,
    };
    let result =
        walk_pe_export_forwarders(&sources, &routes, 0, PeExportQuery::Ordinal(7), cap).unwrap();
    assert_eq!(
        (result.selection_rows, result.text_bytes, result.steps.len()),
        (256, 2805, 256)
    );
    assert_eq!(result.steps.last().unwrap().source_index, 255);
    assert_eq!(
        walk_pe_export_forwarders(
            &sources,
            &routes,
            0,
            PeExportQuery::Ordinal(7),
            PeForwarderWalkLimits {
                max_hops: 255,
                ..cap
            }
        ),
        Err(PeForwarderWalkError::HopLimitExceeded {
            hop: 255,
            limit: 255
        })
    );
}
