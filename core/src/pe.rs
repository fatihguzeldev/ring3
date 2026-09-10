use crate::{FileOffset, RelativeVirtualAddress};

mod base_relocations;
mod certificate_entries;
mod certificates;
mod clr;
mod debug;
mod debug_payloads;
mod delay_imports;
mod delay_lookups;
mod delay_names;
mod export_addresses;
mod export_batch;
mod export_lookup;
mod export_names;
mod exports;
mod header_batch;
mod import_exports;
mod imports;
mod load_config;
mod lookups;
mod optional;
mod resource_data;
mod resource_directories;
mod resource_directory_names;
mod resource_names;
mod resource_payloads;
mod resources;
mod rva;
mod sections;
mod tls;

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
pub use delay_imports::{
    PeDelayImportDescriptor, PeDelayImportError, PeDelayImportTable,
    parse_pe_delay_import_descriptors,
};
pub use delay_lookups::{
    PeDelayImportLookup, PeDelayImportLookupError, PeDelayImportLookupTable,
    parse_pe_delay_import_lookups,
};
pub use delay_names::{
    PeDelayImportName, PeDelayImportNameError, PeDelayImportNameTable, parse_pe_delay_import_names,
};
pub use export_addresses::{
    PeExportAddressEntry, PeExportAddressError, PeExportAddressTable, PeExportTarget,
    parse_pe_export_addresses,
};
pub use export_batch::{
    PeExportBatch, PeExportBatchError, PeExportBatchLimits, lookup_pe_export_batch,
};
pub use export_lookup::{
    PeExportLookup, PeExportLookupError, PeExportQuery, PeExportSelection, lookup_pe_export,
};
pub use export_names::{PeExportName, PeExportNameError, PeExportNameTable, parse_pe_export_names};
pub use exports::{PeExportDirectory, PeExportDirectoryError, parse_pe_export_directory};
pub use header_batch::{
    PeHeaderBatch, PeHeaderBatchError, PeHeaderBatchLimits, parse_pe_header_prefix_batch,
};
pub use import_exports::{PeImportExportBatch, PeImportExportError, lookup_pe_import_exports};
pub use imports::{PeImportDescriptor, PeImportError, parse_pe_import_descriptors};
pub use load_config::{PeLoadConfigError, PeLoadConfigPrefix, parse_pe_load_config_prefix};
pub use lookups::{
    PeImportLookup, PeImportLookupEntry, PeImportLookupError, PeImportSymbol,
    parse_pe_import_lookups,
};
pub use optional::{
    PeDataDirectory, PeDirectoryAddress, PeHeaders, PeOptionalHeader, parse_pe_headers,
};
pub use resource_data::{
    PeResourceDataEntry, PeResourceDataEntryError, PeResourceDataEntryTable,
    PeResourceDataReference, parse_pe_resource_data_entries,
};
pub use resource_directories::{
    PeResourceDirectory, PeResourceDirectoryEntry, PeResourceDirectoryError,
    PeResourceDirectoryGraph, parse_pe_resource_directories,
};
pub use resource_directory_names::{
    PeResourceDirectoryName, PeResourceDirectoryNameError, PeResourceDirectoryNameTable,
    parse_pe_resource_directory_names,
};
pub use resource_names::{
    PeResourceRootName, PeResourceRootNameError, PeResourceRootNameTable,
    parse_pe_resource_root_names,
};
pub use resource_payloads::{
    PeResourcePayload, PeResourcePayloadError, PeResourcePayloadTable, parse_pe_resource_payloads,
};
pub use resources::{
    PeResourceRoot, PeResourceRootEntry, PeResourceRootError, parse_pe_resource_root,
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
