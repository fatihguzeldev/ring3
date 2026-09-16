use super::descriptors::parse_prepared_descriptors;
use super::lookups::lookup_entries;
use crate::pe::rva::PreparedPe;
use crate::{
    PeImportDescriptor, PeImportError, PeImportLookupEntry, PeImportLookupError,
    RelativeVirtualAddress,
};

/// selected descriptor coordinate; does not describe binding state or validity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeImportLookupSource {
    OriginalFirstThunk,
    FirstThunkFallback,
}

/// original descriptor and decoded entries, with names borrowed from input.
/// empty tables retain their selected source. aliased coordinates select the
/// nonzero original-first-thunk coordinate without changing the descriptor.
///
/// ```compile_fail
/// use ring3_core::{PeObservedImportLookup, parse_pe_import_lookups_with_iat_fallback};
///
/// fn escape() -> Vec<PeObservedImportLookup<'static>> {
///     let bytes = vec![0; 64];
///     parse_pe_import_lookups_with_iat_fallback(&bytes).unwrap()
/// }
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeObservedImportLookup<'a> {
    pub descriptor: PeImportDescriptor<'a>,
    pub source: PeImportLookupSource,
    pub entries: Vec<PeImportLookupEntry<'a>>,
}

/// all-or-error result of descriptor admission or selected-table decoding.
/// decoder failures retain the selected table base even when their cause names
/// a different hint/name target. returned nested causes have the same descriptor
/// index and are never `Descriptors` or `LookupTableUnavailable`; caller-created
/// values are not validated by this type.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeImportLookupObservationError {
    Descriptors(PeImportError),
    LookupTableUnavailable {
        descriptor_index: u16,
    },
    Lookup {
        descriptor_index: u16,
        source: PeImportLookupSource,
        start: RelativeVirtualAddress,
        cause: PeImportLookupError,
    },
}

/// observes static lookup-shaped bytes, with an explicit zero-oft iat fallback.
///
/// all descriptors and dll names validate before any lookup is selected. nonzero
/// original-first-thunk always wins and is never rescued by an iat after failure;
/// only a zero original-first-thunk selects a nonzero first-thunk coordinate.
/// timestamps, bound-import records and table contents do not select the source.
/// names borrow the unchanged input; successful entries preserve raw values and
/// their actual lookup rvas and file offsets. no partial tables escape an error.
///
/// the shared decoder admits 1024 entries per dll and 4096 total, plus 1024 bytes
/// per symbol name and 65,536 total name bytes, including nul and repeated scans.
/// hint bytes and dll-name budgets are separate. complete range reads precede
/// entry limits; fetched zero terminators precede local then global entry limits.
/// local then global name limits precede hint/name reads. these logical limits
/// do not bound input acquisition, process memory, allocation failure or elapsed
/// time, and provide no cancellation mechanism.
///
/// successful decoding does not establish unbound state, windows acceptance,
/// loadability or provider identity: address-origin bytes can look like lookup
/// metadata. this reader never writes the iat, binds imports or loads code. the
/// strict [`super::parse_pe_import_lookups`] and existing consumers are unchanged.
///
/// ```
/// use ring3_core::parse_pe_import_lookups_with_iat_fallback;
/// assert!(parse_pe_import_lookups_with_iat_fallback(&[]).is_err());
/// ```
///
/// # errors
/// returns complete descriptor errors first, `LookupTableUnavailable` when both
/// coordinates are zero, or a decoder error with its descriptor index, selected
/// source, table base and unchanged cause.
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
#[must_use = "lookup observations or their parse error should be inspected"]
pub fn parse_pe_import_lookups_with_iat_fallback(
    bytes: &[u8],
) -> Result<Vec<PeObservedImportLookup<'_>>, PeImportLookupObservationError> {
    let prepared = PreparedPe::new(bytes)
        .map_err(|error| PeImportLookupObservationError::Descriptors(PeImportError::Base(error)))?;
    let descriptors = parse_prepared_descriptors(&prepared)
        .map_err(PeImportLookupObservationError::Descriptors)?;
    let mut lookups = Vec::new();
    let mut total_entries = 0;
    let mut total_name_bytes = 0;
    for (descriptor_index, descriptor) in (0_u16..).zip(descriptors) {
        let (source, start) = if descriptor.import_lookup_table_rva.get() != 0 {
            (
                PeImportLookupSource::OriginalFirstThunk,
                descriptor.import_lookup_table_rva,
            )
        } else if descriptor.import_address_table_rva.get() != 0 {
            (
                PeImportLookupSource::FirstThunkFallback,
                descriptor.import_address_table_rva,
            )
        } else {
            return Err(PeImportLookupObservationError::LookupTableUnavailable {
                descriptor_index,
            });
        };
        let entries = lookup_entries(
            &prepared,
            descriptor_index,
            start,
            &mut total_entries,
            &mut total_name_bytes,
        )
        .map_err(|cause| PeImportLookupObservationError::Lookup {
            descriptor_index,
            source,
            start,
            cause,
        })?;
        lookups.push(PeObservedImportLookup {
            descriptor,
            source,
            entries,
        });
    }
    Ok(lookups)
}
