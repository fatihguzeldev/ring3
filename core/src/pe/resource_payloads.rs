use super::resource_data::parse_prepared_resource_data_entries;
use super::rva::PreparedPe;
use super::{
    PeFileRange, PeResourceDataEntryError, PeResourceDataEntryTable, PeResourceDirectoryError,
    PeResourceRootError, PeRvaError,
};
use crate::RelativeVirtualAddress;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeResourcePayload<'a> {
    pub data_entry_index: u16,
    /// no physical coordinate or source is assigned to a zero-size payload.
    pub range: Option<PeFileRange<'a>>,
}

/// borrowed payload ranges indexed by their distinct raw data records.
///
/// ```compile_fail
/// use ring3_core::{PeResourcePayloadTable, parse_pe_resource_payloads};
///
/// fn escape() -> Option<PeResourcePayloadTable<'static>> {
///     let bytes = vec![0; 64];
///     parse_pe_resource_payloads(&bytes).unwrap()
/// }
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeResourcePayloadTable<'a> {
    pub data_entry_table: PeResourceDataEntryTable,
    pub payloads: Vec<PeResourcePayload<'a>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeResourcePayloadError {
    Data(PeResourceDataEntryError),
    PayloadRange {
        data_entry_index: u16,
        start: RelativeVirtualAddress,
        length: u32,
        cause: PeRvaError,
    },
}

/// borrows one conservative raw range per distinct resource data record.
/// zero-size records retain their raw rva in the data table and have no range.
/// nonempty payloads use image rvas, including headers or other sections. shared
/// records do not multiply views; distinct records may share or overlap bytes.
/// graph entry limits bound view metadata; payload bytes are not copied, scanned,
/// decoded or interpreted. no additional payload byte budget is imposed.
///
/// # errors
/// validates the complete graph and data table before any payload. then resolves
/// nonempty ranges in data-record order, attributing the first failure by index
/// and preserving its complete conservative resolver error.
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn parse_pe_resource_payloads(
    bytes: &[u8],
) -> Result<Option<PeResourcePayloadTable<'_>>, PeResourcePayloadError> {
    let prepared = PreparedPe::new(bytes).map_err(|cause| {
        PeResourcePayloadError::Data(PeResourceDataEntryError::Graph(
            PeResourceDirectoryError::Root(PeResourceRootError::Base(cause)),
        ))
    })?;
    let Some(data_entry_table) =
        parse_prepared_resource_data_entries(&prepared).map_err(PeResourcePayloadError::Data)?
    else {
        return Ok(None);
    };
    let mut payloads = Vec::new();
    for (data_entry_index, entry) in (0_u16..).zip(&data_entry_table.data_entries) {
        let range = if entry.payload_size == 0 {
            None
        } else {
            Some(
                prepared
                    .resolve(entry.payload_rva, entry.payload_size)
                    .map_err(|cause| PeResourcePayloadError::PayloadRange {
                        data_entry_index,
                        start: entry.payload_rva,
                        length: entry.payload_size,
                        cause,
                    })?,
            )
        };
        payloads.push(PeResourcePayload {
            data_entry_index,
            range,
        });
    }
    Ok(Some(PeResourcePayloadTable {
        data_entry_table,
        payloads,
    }))
}
