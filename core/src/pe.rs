use crate::{FileOffset, RelativeVirtualAddress};

mod amd64_exceptions;
mod architecture_declarations;
mod base_relocations;
mod certificate_entries;
mod certificates;
mod clr;
mod debug;
mod debug_payloads;
mod declared_evidence;
mod exports;
mod fingerprints;
mod header_batch;
mod imports;
mod load_config;
mod module_evidence;
mod optional;
mod resources;
mod rva;
mod sections;
mod tls;

pub use amd64_exceptions::{
    PeAmd64ExceptionEntry, PeAmd64ExceptionError, PeAmd64ExceptionTable, PeAmd64UnwindInfoV1,
    PeAmd64UnwindInfoV1Error, PeAmd64UnwindTailV1, parse_pe_amd64_exception_functions,
    parse_pe_amd64_unwind_info_v1,
};
pub use architecture_declarations::{
    PeArchitectureDeclarations, PeClrArchitectureDeclarations, PeCoffArchitectureDeclarations,
    PeFlagBit, describe_pe_architecture_declarations,
};
pub use base_relocations::{
    PeBaseRelocationBlock, PeBaseRelocationError, parse_pe_base_relocation_blocks,
};
pub use certificate_entries::{
    PeCertificateEntry, PeCertificateEntryError, PeCertificateEntryTable,
    parse_pe_certificate_entries,
};
pub use certificates::{PeCertificateError, PeCertificateTable, parse_pe_certificate_table};
pub use clr::{PeClrDataDirectory, PeClrError, PeClrHeader, parse_pe_clr_header};
pub use debug::{
    PeDebugDirectoryEntry, PeDebugDirectoryError, PeDebugDirectoryTable, parse_pe_debug_directory,
};
pub use debug_payloads::{
    PeDebugPayload, PeDebugPayloadError, PeDebugPayloadRange, PeDebugPayloadTable,
    parse_pe_debug_payloads,
};
pub use declared_evidence::{
    PeClrDescriptorEvidence, PeClrHeaderEvidence, PeDeclaredEvidence, PeFieldEvidence,
    PeHeaderPrefixEvidence, PeOptionalHeaderEvidence, inspect_pe_declared_evidence,
};
pub use exports::{
    PeExportAddressEntry, PeExportAddressError, PeExportAddressTable, PeExportBatch,
    PeExportBatchError, PeExportBatchLimits, PeExportDirectory, PeExportDirectoryError,
    PeExportEvidence, PeExportEvidenceError, PeExportEvidenceLimits, PeExportLookup,
    PeExportLookupError, PeExportName, PeExportNameError, PeExportNameTable, PeExportQuery,
    PeExportSelection, PeExportTarget, PeForwarderHop, PeForwarderQuery, PeForwarderRequest,
    PeForwarderRequestError, PeForwarderRoute, PeForwarderStep, PeForwarderSymbol,
    PeForwarderTextContext, PeForwarderWalk, PeForwarderWalkError, PeForwarderWalkLimits,
    PeOwnedExportAddressEntry, PeOwnedExportAddressTable, PeOwnedExportName,
    PeOwnedExportNameTable, PeOwnedExportTarget, decode_pe_forwarder_request, inspect_pe_exports,
    lookup_pe_export, lookup_pe_export_batch, parse_pe_export_addresses, parse_pe_export_directory,
    parse_pe_export_names, walk_pe_export_forwarders,
};
pub use exports::{PeExportEvidenceBatch, lookup_pe_export_evidence_batch};
pub use exports::{
    PeExportEvidenceLookupError, PeExportEvidenceLookupLimits, lookup_pe_export_evidence,
};
pub use fingerprints::{
    PeFingerprintError, PeFingerprintedEvidence, fingerprint_pe_declared_evidence,
};
pub(crate) use header_batch::preflight as admit_pe_input_lengths;
pub use header_batch::{
    PeHeaderBatch, PeHeaderBatchError, PeHeaderBatchLimits, parse_pe_header_prefix_batch,
};
pub use imports::{
    PeDelayImportDescriptor, PeDelayImportError, PeDelayImportEvidence, PeDelayImportEvidenceError,
    PeDelayImportEvidenceLimits, PeDelayImportExportBatch, PeDelayImportExportError,
    PeDelayImportLookup, PeDelayImportLookupError, PeDelayImportLookupTable, PeDelayImportName,
    PeDelayImportNameError, PeDelayImportNameTable, PeDelayImportTable, PeImportDescriptor,
    PeImportError, PeImportExportBatch, PeImportExportError, PeImportLookup, PeImportLookupEntry,
    PeImportLookupError, PeImportSymbol, PeOwnedDelayImportLookup, PeOwnedDelayImportLookupTable,
    PeOwnedDelayImportName, PeOwnedDelayImportNameTable, inspect_pe_delay_imports,
    lookup_pe_delay_import_exports, lookup_pe_delay_import_exports_with_provider,
    lookup_pe_import_exports, lookup_pe_import_exports_with_provider,
    parse_pe_delay_import_descriptors, parse_pe_delay_import_lookups, parse_pe_delay_import_names,
    parse_pe_import_descriptors, parse_pe_import_lookups,
};
pub use imports::{PeImportEvidenceExportBatch, lookup_pe_import_evidence_exports};
pub use imports::{
    PeOwnedImportDescriptor, PeOwnedImportLookup, PeOwnedImportLookupEntry, PeOwnedImportSymbol,
    PeStaticImportEvidence, PeStaticImportEvidenceError, PeStaticImportEvidenceLimits,
    inspect_pe_static_imports,
};
pub use load_config::{PeLoadConfigError, PeLoadConfigPrefix, parse_pe_load_config_prefix};
pub use module_evidence::{
    PeModuleEvidence, PeModuleEvidenceLimits, PeModuleOutputLimits, inspect_pe_module_evidence,
};
pub use optional::{
    PeDataDirectory, PeDirectoryAddress, PeHeaders, PeOptionalHeader, parse_pe_headers,
};
pub use resources::{
    PeResourceDataEntry, PeResourceDataEntryError, PeResourceDataEntryTable,
    PeResourceDataReference, PeResourceDirectory, PeResourceDirectoryEntry,
    PeResourceDirectoryError, PeResourceDirectoryGraph, PeResourceDirectoryName,
    PeResourceDirectoryNameError, PeResourceDirectoryNameTable, PeResourcePayload,
    PeResourcePayloadError, PeResourcePayloadTable, PeResourceRoot, PeResourceRootEntry,
    PeResourceRootError, PeResourceRootName, PeResourceRootNameError, PeResourceRootNameTable,
    parse_pe_resource_data_entries, parse_pe_resource_directories,
    parse_pe_resource_directory_names, parse_pe_resource_payloads, parse_pe_resource_root,
    parse_pe_resource_root_names,
};
pub use rva::{PeFileRange, PeFileRangeSource, PeRvaError, resolve_pe_file_range};
pub use sections::{PeSection, PeSectionTable, parse_pe_sections};
pub use tls::{PeTlsDirectory, PeTlsDirectoryError, parse_pe_tls_directory};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeKind {
    Pe32,
    Pe32Plus,
}

/// recognized header prefix metadata, not a validated or loadable image.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeHeaderPrefix {
    pub pe_offset: FileOffset,
    pub machine: u16,
    pub number_of_sections: u16,
    pub characteristics: u16,
    pub size_of_optional_header: u16,
    pub kind: PeKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeHeaderError {
    OutOfBounds {
        offset: FileOffset,
        needed: u64,
        available: u64,
    },
    InvalidDosSignature {
        offset: FileOffset,
    },
    InvalidPeSignature {
        offset: FileOffset,
    },
    OptionalHeaderTooShort {
        offset: FileOffset,
        declared_size: u16,
    },
    UnsupportedOptionalMagic {
        offset: FileOffset,
        magic: u16,
    },
    MachineKindMismatch {
        offset: FileOffset,
        machine: u16,
        kind: PeKind,
    },
    OptionalHeaderExtentTooShort {
        offset: FileOffset,
        required: u64,
        declared: u16,
    },
    DirectoryLimitExceeded {
        offset: FileOffset,
        count: u32,
        limit: u32,
    },
    SectionLimitExceeded {
        offset: FileOffset,
        count: u16,
        limit: u16,
    },
    SectionTableOutOfBounds {
        offset: FileOffset,
        needed: u64,
        available: u64,
    },
    SectionRawDataOutOfBounds {
        section_index: u16,
        section_offset: FileOffset,
        offset: FileOffset,
        needed: u64,
        available: u64,
    },
    VirtualRangeOverflow {
        section_index: u16,
        offset: FileOffset,
        virtual_address: RelativeVirtualAddress,
        virtual_size: u32,
    },
}

struct Reader<'a> {
    bytes: &'a [u8],
}

impl<'a> Reader<'a> {
    fn read(
        &self,
        offset: FileOffset,
        needed: u64,
    ) -> Result<(&'a [u8], FileOffset), PeHeaderError> {
        let length = self.bytes.len() as u64;
        let error = PeHeaderError::OutOfBounds {
            offset,
            needed,
            available: length.saturating_sub(offset.get()),
        };
        let end = offset
            .checked_add(needed)
            .filter(|end| end.get() <= length)
            .ok_or(error)?;
        let start_index = usize::try_from(offset.get()).map_err(|_| error)?;
        let end_index = usize::try_from(end.get()).map_err(|_| error)?;
        let bytes = self.bytes.get(start_index..end_index).ok_or(error)?;
        Ok((bytes, end))
    }
}

/// recognizes the prefix while preserving unclassified machine and count fields.
///
/// # errors
/// rejects missing bytes, bad signatures, insufficient declared magic space,
/// unknown optional magic, and known i386/amd64 layout mismatches.
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn parse_pe_header_prefix(bytes: &[u8]) -> Result<PeHeaderPrefix, PeHeaderError> {
    let reader = Reader { bytes };
    let (dos_signature, _) = reader.read(FileOffset::new(0), 2)?;
    if dos_signature != b"MZ" {
        return Err(PeHeaderError::InvalidDosSignature {
            offset: FileOffset::new(0),
        });
    }
    let (lfanew, _) = reader.read(FileOffset::new(0x3c), 4)?;
    let pe_offset = FileOffset::new(u64::from(u32::from_le_bytes([
        lfanew[0], lfanew[1], lfanew[2], lfanew[3],
    ])));
    let (signature, coff_offset) = reader.read(pe_offset, 4)?;
    if signature != b"PE\0\0" {
        return Err(PeHeaderError::InvalidPeSignature { offset: pe_offset });
    }
    let (coff, optional_offset) = reader.read(coff_offset, 20)?;
    let machine = u16::from_le_bytes([coff[0], coff[1]]);
    let number_of_sections = u16::from_le_bytes([coff[2], coff[3]]);
    let size_of_optional_header = u16::from_le_bytes([coff[16], coff[17]]);
    let characteristics = u16::from_le_bytes([coff[18], coff[19]]);
    if size_of_optional_header < 2 {
        return Err(PeHeaderError::OptionalHeaderTooShort {
            offset: optional_offset,
            declared_size: size_of_optional_header,
        });
    }
    let (optional, _) = reader.read(optional_offset, u64::from(size_of_optional_header))?;
    let magic = u16::from_le_bytes([optional[0], optional[1]]);
    let kind = match magic {
        0x10b => PeKind::Pe32,
        0x20b => PeKind::Pe32Plus,
        _ => {
            return Err(PeHeaderError::UnsupportedOptionalMagic {
                offset: optional_offset,
                magic,
            });
        }
    };
    if matches!(
        (machine, kind),
        (0x14c, PeKind::Pe32Plus) | (0x8664, PeKind::Pe32)
    ) {
        return Err(PeHeaderError::MachineKindMismatch {
            offset: coff_offset,
            machine,
            kind,
        });
    }
    Ok(PeHeaderPrefix {
        pe_offset,
        machine,
        number_of_sections,
        characteristics,
        size_of_optional_header,
        kind,
    })
}

pub use imports::{PeDelayImportEvidenceExportBatch, lookup_pe_delay_import_evidence_exports};

pub use exports::{
    PeForwarderEvidenceWalkBatch, PeForwarderWalkBatchError, PeForwarderWalkBatchLimits,
    PeForwarderWalkBatchMetric, walk_pe_export_evidence_forwarders_batch,
};
pub use exports::{PeForwarderEvidenceWalkError, walk_pe_export_evidence_forwarders};

pub use imports::{
    PeBoundForwarderRef, PeBoundImportDescriptor, PeBoundImportError, PeBoundImportTable,
    parse_pe_bound_import_descriptors,
};

#[cfg(test)]
mod tests {
    use super::{FileOffset, PeHeaderError, Reader};

    #[test]
    fn reader_rejects_coordinate_overflow_and_offsets_past_host_width() {
        let reader = Reader { bytes: &[1, 2, 3] };
        for (offset, needed) in [(u64::MAX, 1), (1, u64::MAX), (0x1_0000_0000, 1)] {
            assert_eq!(
                reader.read(FileOffset::new(offset), needed),
                Err(PeHeaderError::OutOfBounds {
                    offset: FileOffset::new(offset),
                    needed,
                    available: 3_u64.saturating_sub(offset),
                })
            );
        }
    }

    #[test]
    fn reader_checks_even_empty_ranges_and_returns_exact_end() {
        let reader = Reader { bytes: &[1, 2, 3] };
        assert_eq!(
            reader.read(FileOffset::new(1), 2),
            Ok((&[2, 3][..], FileOffset::new(3)))
        );
        assert_eq!(
            reader.read(FileOffset::new(3), 0),
            Ok((&[][..], FileOffset::new(3)))
        );
        assert_eq!(
            reader.read(FileOffset::new(4), 0),
            Err(PeHeaderError::OutOfBounds {
                offset: FileOffset::new(4),
                needed: 0,
                available: 0
            })
        );
    }
}
