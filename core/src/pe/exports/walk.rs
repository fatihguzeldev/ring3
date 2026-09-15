use super::{
    PeExportBatchError, PeExportBatchLimits, PeExportLookup, PeExportLookupError, PeExportQuery,
    PeExportSelection, PeExportTarget, PeForwarderRequest, PeForwarderRequestError,
    PeForwarderSymbol, decode_pe_forwarder_request,
};
use std::collections::BTreeMap;

/// caller-selected logical work limits; these do not cap image bytes or process memory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeForwarderWalkLimits {
    pub max_sources: u64,
    pub max_routes: u64,
    pub max_hops: u64,
    pub max_selection_rows: u64,
    pub max_text_bytes: u64,
}

/// exact module text is routed in the context of a source's position in this call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeForwarderRoute<'a> {
    pub source_index: u32,
    pub requested_module: &'a str,
    pub destination_index: u32,
}

/// owns name text or an ordinal value; raw ordinal digits remain in the hop request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PeForwarderQuery {
    Name(String),
    Ordinal(u32),
}

impl PeForwarderQuery {
    fn from_input(query: PeExportQuery<'_>) -> Self {
        match query {
            PeExportQuery::Name(name) => Self::Name(name.to_owned()),
            PeExportQuery::Ordinal(value) => Self::Ordinal(value),
        }
    }

    fn borrowed(&self) -> PeExportQuery<'_> {
        match self {
            Self::Name(name) => PeExportQuery::Name(name),
            Self::Ordinal(value) => PeExportQuery::Ordinal(*value),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeForwarderHop<'a> {
    pub request: PeForwarderRequest<'a>,
    pub destination_index: u32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeForwarderStep<'a> {
    pub source_index: u32,
    pub query: PeForwarderQuery,
    pub selection: PeExportSelection<'a>,
    pub forwarder: Option<PeForwarderHop<'a>>,
}

/// complete ordered metadata, borrowing only source images; errors contain no partial walk.
///
/// ```compile_fail
/// use ring3_core::{PeExportQuery, PeForwarderWalk, PeForwarderWalkLimits, walk_pe_export_forwarders};
/// fn escape() -> PeForwarderWalk<'static> {
///     let image = vec![0; 64];
///     walk_pe_export_forwarders(&[&image], &[], 0, PeExportQuery::Ordinal(1),
///         PeForwarderWalkLimits { max_sources: 1, max_routes: 0, max_hops: 1,
///             max_selection_rows: 1, max_text_bytes: 0 }).unwrap()
/// }
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeForwarderWalk<'a> {
    pub selection_rows: u64,
    pub text_bytes: u64,
    pub steps: Vec<PeForwarderStep<'a>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeForwarderTextContext {
    RouteToken { index: usize },
    RootName,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PeForwarderWalkError<'a> {
    SourceCountExceeded {
        count: u64,
        limit: u64,
    },
    SourceIndexSpaceExceeded {
        count: u64,
        maximum: u64,
    },
    RouteCountExceeded {
        count: u64,
        limit: u64,
    },
    TextBytesOverflow {
        context: PeForwarderTextContext,
        total: u64,
        bytes: u64,
    },
    TextBytesExceeded {
        context: PeForwarderTextContext,
        total: u64,
        limit: u64,
    },
    RouteSourceOutOfRange {
        index: usize,
        source_index: u32,
        source_count: u64,
    },
    RouteDestinationOutOfRange {
        index: usize,
        destination_index: u32,
        source_count: u64,
    },
    DuplicateRoute {
        first: usize,
        index: usize,
        source_index: u32,
        requested_module: String,
    },
    RootSourceOutOfRange {
        source_index: u32,
        source_count: u64,
    },
    HopLimitExceeded {
        hop: u64,
        limit: u64,
    },
    SelectionRows {
        hop: u64,
        source_index: u32,
        used: u64,
        cause: PeExportBatchError,
    },
    SelectionRowsOverflow {
        hop: u64,
        source_index: u32,
        total: u64,
        rows: u64,
    },
    Provider {
        hop: u64,
        source_index: u32,
        cause: PeExportLookupError,
    },
    Cycle {
        first_hop: u64,
        hop: u64,
        source_index: u32,
        table_index: u32,
    },
    Decode {
        hop: u64,
        source_index: u32,
        used_text_bytes: u64,
        cause: PeForwarderRequestError,
    },
    MissingRoute {
        hop: u64,
        source_index: u32,
        request: PeForwarderRequest<'a>,
    },
}

fn check_source_count(count: u64, limit: u64) -> Result<(), PeForwarderWalkError<'static>> {
    if count > limit {
        return Err(PeForwarderWalkError::SourceCountExceeded { count, limit });
    }
    let maximum = 1_u64 << 32;
    if count > maximum {
        return Err(PeForwarderWalkError::SourceIndexSpaceExceeded { count, maximum });
    }
    Ok(())
}

fn charge_text(
    total: u64,
    bytes: u64,
    limit: u64,
    context: PeForwarderTextContext,
) -> Result<u64, PeForwarderWalkError<'static>> {
    let next = total
        .checked_add(bytes)
        .ok_or(PeForwarderWalkError::TextBytesOverflow {
            context,
            total,
            bytes,
        })?;
    if next > limit {
        return Err(PeForwarderWalkError::TextBytesExceeded {
            context,
            total: next,
            limit,
        });
    }
    Ok(next)
}

struct AdmittedRoutes<'a> {
    bindings: BTreeMap<(u32, &'a str), (usize, u32)>,
    text_bytes: u64,
}

fn admit_inputs<'a>(
    source_count: u64,
    routes: &[PeForwarderRoute<'a>],
    root_source_index: u32,
    root_query: PeExportQuery<'_>,
    limits: PeForwarderWalkLimits,
) -> Result<AdmittedRoutes<'a>, PeForwarderWalkError<'static>> {
    check_source_count(source_count, limits.max_sources)?;
    if routes.len() as u64 > limits.max_routes {
        return Err(PeForwarderWalkError::RouteCountExceeded {
            count: routes.len() as u64,
            limit: limits.max_routes,
        });
    }
    let mut text_bytes = 0;
    for (index, route) in routes.iter().enumerate() {
        text_bytes = charge_text(
            text_bytes,
            route.requested_module.len() as u64,
            limits.max_text_bytes,
            PeForwarderTextContext::RouteToken { index },
        )?;
    }
    if let PeExportQuery::Name(name) = root_query {
        text_bytes = charge_text(
            text_bytes,
            name.len() as u64,
            limits.max_text_bytes,
            PeForwarderTextContext::RootName,
        )?;
    }
    let mut bindings = BTreeMap::new();
    for (index, route) in routes.iter().enumerate() {
        if u64::from(route.source_index) >= source_count {
            return Err(PeForwarderWalkError::RouteSourceOutOfRange {
                index,
                source_index: route.source_index,
                source_count,
            });
        }
        if u64::from(route.destination_index) >= source_count {
            return Err(PeForwarderWalkError::RouteDestinationOutOfRange {
                index,
                destination_index: route.destination_index,
                source_count,
            });
        }
        if let Some(&(first, _)) = bindings.get(&(route.source_index, route.requested_module)) {
            return Err(PeForwarderWalkError::DuplicateRoute {
                first,
                index,
                source_index: route.source_index,
                requested_module: route.requested_module.to_owned(),
            });
        }
        bindings.insert(
            (route.source_index, route.requested_module),
            (index, route.destination_index),
        );
    }
    if u64::from(root_source_index) >= source_count {
        return Err(PeForwarderWalkError::RootSourceOutOfRange {
            source_index: root_source_index,
            source_count,
        });
    }
    Ok(AdmittedRoutes {
        bindings,
        text_bytes,
    })
}

fn lookup_step<'image>(
    owner: &mut PeExportLookup<'image>,
    query: PeExportQuery<'_>,
    hop: u64,
    source_index: u32,
    selection_rows: u64,
    max_rows: u64,
) -> Result<(PeExportSelection<'image>, u64), PeForwarderWalkError<'image>> {
    let mut batch = owner
        .lookup_batch(
            &[query],
            PeExportBatchLimits {
                max_queries: 1,
                max_selection_rows: max_rows - selection_rows,
            },
        )
        .map_err(|cause| PeForwarderWalkError::SelectionRows {
            hop,
            source_index,
            used: selection_rows,
            cause,
        })?;
    let selection_rows = selection_rows.checked_add(batch.selection_rows).ok_or(
        PeForwarderWalkError::SelectionRowsOverflow {
            hop,
            source_index,
            total: selection_rows,
            rows: batch.selection_rows,
        },
    )?;
    let selection = batch
        .selections
        .pop()
        .expect("one admitted query has one result")
        .map_err(|cause| PeForwarderWalkError::Provider {
            hop,
            source_index,
            cause,
        })?;
    Ok((selection, selection_rows))
}

/// walks one export query through explicit source-context routes without resolving modules.
/// source indices are call-local positions: identical bytes at two positions stay distinct.
/// each call creates fresh lazy owners and reuses them when revisiting a source.
/// name queries are copied; raw metadata borrows images, not routes, queries or owners.
///
/// text charges every route token, then a named root, then each admitted forwarder text.
/// hops count lookup attempts; rows follow `PeExportLookup::lookup_batch` accounting.
/// nonselected, ambiguous, empty and raw-rva selections terminate; target rvas are not followed.
/// a cycle is a repeated (source position, export address table index), including aliases.
/// no case folding, extension inference, path search or windows compatibility is implied.
///
/// # panics
/// only if an internal decoder or single-query batch invariant is violated.
///
/// # errors
/// admission checks source count/representability, route count, aggregate text, each
/// route's source/destination/duplicate key, then root validity, before any pe read.
/// traversal checks hops, lookup/row admission, cycle, decoding, then exact routing.
/// text overflow precedes its limit; all routes, including unused ones, are admitted.
/// repeated entries pay lookup/row cost before cycle refusal, without recharging text.
/// refusal discards prior steps. limits do not cap temporary allocations or provide
/// cancellation, acquisition limits, allocator-failure recovery or a time deadline.
///
/// ```
/// use ring3_core::{PeExportQuery, PeForwarderWalkError, PeForwarderWalkLimits, walk_pe_export_forwarders};
/// let result = walk_pe_export_forwarders(&[b""], &[], 0, PeExportQuery::Ordinal(1),
///     PeForwarderWalkLimits { max_sources: 1, max_routes: 0, max_hops: 0,
///         max_selection_rows: 0, max_text_bytes: 0 });
/// assert_eq!(result, Err(PeForwarderWalkError::HopLimitExceeded { hop: 0, limit: 0 }));
/// ```
#[expect(
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    reason = "project documentation headings are lower case"
)]
pub fn walk_pe_export_forwarders<'image>(
    sources: &[&'image [u8]],
    routes: &[PeForwarderRoute<'_>],
    root_source_index: u32,
    root_query: PeExportQuery<'_>,
    limits: PeForwarderWalkLimits,
) -> Result<PeForwarderWalk<'image>, PeForwarderWalkError<'image>> {
    let AdmittedRoutes {
        bindings,
        mut text_bytes,
    } = admit_inputs(
        sources.len() as u64,
        routes,
        root_source_index,
        root_query,
        limits,
    )?;
    let mut owners: Vec<_> = sources
        .iter()
        .map(|&bytes| PeExportLookup::new(bytes))
        .collect();
    let mut visited = BTreeMap::new();
    let mut steps = Vec::new();
    let mut selection_rows = 0_u64;
    let mut source_index = root_source_index;
    let mut query = PeForwarderQuery::from_input(root_query);
    loop {
        let hop = steps.len() as u64;
        if hop >= limits.max_hops {
            return Err(PeForwarderWalkError::HopLimitExceeded {
                hop,
                limit: limits.max_hops,
            });
        }
        let (selection, total) = lookup_step(
            &mut owners[source_index as usize],
            query.borrowed(),
            hop,
            source_index,
            selection_rows,
            limits.max_selection_rows,
        )?;
        selection_rows = total;
        let address = match &selection {
            PeExportSelection::Selected { address, .. } => Some(*address),
            _ => None,
        };
        let mut step = PeForwarderStep {
            source_index,
            query: query.clone(),
            selection,
            forwarder: None,
        };
        let Some(address) = address else {
            steps.push(step);
            return Ok(PeForwarderWalk {
                selection_rows,
                text_bytes,
                steps,
            });
        };
        if let Some(&first_hop) = visited.get(&(source_index, address.table_index)) {
            return Err(PeForwarderWalkError::Cycle {
                first_hop,
                hop,
                source_index,
                table_index: address.table_index,
            });
        }
        visited.insert((source_index, address.table_index), hop);
        let PeExportTarget::Forwarder { text: raw, .. } = address.target else {
            steps.push(step);
            return Ok(PeForwarderWalk {
                selection_rows,
                text_bytes,
                steps,
            });
        };
        let request = decode_pe_forwarder_request(raw, limits.max_text_bytes - text_bytes)
            .map_err(|cause| PeForwarderWalkError::Decode {
                hop,
                source_index,
                used_text_bytes: text_bytes,
                cause,
            })?;
        text_bytes = text_bytes
            .checked_add(raw.len() as u64)
            .expect("decoder admitted remaining text bytes");
        let Some(&(_, destination_index)) = bindings.get(&(source_index, request.module)) else {
            return Err(PeForwarderWalkError::MissingRoute {
                hop,
                source_index,
                request,
            });
        };
        step.forwarder = Some(PeForwarderHop {
            request,
            destination_index,
        });
        steps.push(step);
        source_index = destination_index;
        query = match request.symbol {
            PeForwarderSymbol::Name(name) => PeForwarderQuery::Name(name.to_owned()),
            PeForwarderSymbol::Ordinal { value, .. } => PeForwarderQuery::Ordinal(value),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::{PeForwarderTextContext, PeForwarderWalkError, charge_text, check_source_count};

    #[test]
    fn synthetic_text_boundaries_preserve_admission_order() {
        let cases = [
            (0, 0, 0, Ok(0)),
            (0, 1, 0, Err((false, 1, 0))),
            (u64::MAX, 0, u64::MAX, Ok(u64::MAX)),
            (u64::MAX, 1, u64::MAX, Err((true, u64::MAX, 1))),
            (u64::MAX - 1, 1, u64::MAX, Ok(u64::MAX)),
            (u64::MAX - 1, 2, u64::MAX, Err((true, u64::MAX - 1, 2))),
            (10, 3, 12, Err((false, 13, 12))),
            (10, 2, 12, Ok(12)),
            (u64::MAX, 1, 0, Err((true, u64::MAX, 1))),
            (0, u64::MAX, u64::MAX, Ok(u64::MAX)),
        ];
        for (index, (total, bytes, limit, expected)) in cases.into_iter().enumerate() {
            let context = PeForwarderTextContext::RouteToken { index };
            let expected = expected.map_err(|(overflow, total, bound)| {
                if overflow {
                    PeForwarderWalkError::TextBytesOverflow {
                        context,
                        total,
                        bytes: bound,
                    }
                } else {
                    PeForwarderWalkError::TextBytesExceeded {
                        context,
                        total,
                        limit: bound,
                    }
                }
            });
            assert_eq!(charge_text(total, bytes, limit, context), expected);
        }
    }

    #[test]
    fn synthetic_sources_boundaries_preserve_admission_order() {
        let domain = 1_u64 << 32;
        for (count, limit, expected) in [
            (0, 0, Ok(())),
            (
                1,
                0,
                Err(PeForwarderWalkError::SourceCountExceeded { count: 1, limit: 0 }),
            ),
            (domain, u64::MAX, Ok(())),
            (
                domain + 1,
                u64::MAX,
                Err(PeForwarderWalkError::SourceIndexSpaceExceeded {
                    count: domain + 1,
                    maximum: domain,
                }),
            ),
            (
                domain + 1,
                domain,
                Err(PeForwarderWalkError::SourceCountExceeded {
                    count: domain + 1,
                    limit: domain,
                }),
            ),
            (
                u64::MAX,
                u64::MAX,
                Err(PeForwarderWalkError::SourceIndexSpaceExceeded {
                    count: u64::MAX,
                    maximum: domain,
                }),
            ),
            (domain - 1, domain - 1, Ok(())),
        ] {
            assert_eq!(check_source_count(count, limit), expected);
        }
    }
}
