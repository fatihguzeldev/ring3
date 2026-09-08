use super::imports::parse_prepared_descriptors;
use super::optional::{read_u16, read_u32};
use super::rva::PreparedPe;
use super::{PeImportDescriptor, PeImportError, PeKind, PeRvaError};
use crate::{FileOffset, RelativeVirtualAddress};

const ENTRY_LIMIT: u16 = 1024;
const TOTAL_ENTRY_LIMIT: u32 = 4096;
const NAME_LENGTH_LIMIT: u32 = 1024;
const NAME_SCAN_BUDGET: u32 = 65_536;

/// symbol identity metadata; does not classify functions or data.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeImportSymbol<'a> {
    Ordinal(u16),
    ByName {
        hint_name_rva: RelativeVirtualAddress,
        hint: u16,
        name: &'a str,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeImportLookupEntry<'a> {
    pub lookup_rva: RelativeVirtualAddress,
    pub lookup_file_offset: FileOffset,
    pub raw_value: u64,
    pub symbol: PeImportSymbol<'a>,
}

/// explicit lookup metadata with descriptor and symbol names borrowed from input.
///
/// ```compile_fail
/// use ring3_core::{PeImportLookup, parse_pe_import_lookups};
///
/// fn escape() -> Vec<PeImportLookup<'static>> {
///     let bytes = vec![0; 64];
///     parse_pe_import_lookups(&bytes).unwrap()
/// }
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeImportLookup<'a> {
    pub descriptor: PeImportDescriptor<'a>,
    pub entries: Vec<PeImportLookupEntry<'a>>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeImportLookupError {
    Descriptors(PeImportError),
    LookupTableUnavailable {
        descriptor_index: u16,
    },
    LookupRange {
        descriptor_index: u16,
        entry_index: u16,
        start: RelativeVirtualAddress,
        length: u32,
        cause: PeRvaError,
    },
    EntryLimitExceeded {
        descriptor_index: u16,
        entry_index: u16,
        limit: u16,
    },
    TotalEntryLimitExceeded {
        descriptor_index: u16,
        entry_index: u16,
        limit: u32,
    },
    InvalidOrdinalEncoding {
        descriptor_index: u16,
        entry_index: u16,
        raw_value: u64,
        kind: PeKind,
    },
    InvalidNameEncoding {
        descriptor_index: u16,
        entry_index: u16,
        raw_value: u64,
        kind: PeKind,
    },
    HintNameRange {
        descriptor_index: u16,
        entry_index: u16,
        hint_name_rva: RelativeVirtualAddress,
        offset: u32,
        cause: PeRvaError,
    },
    EmptySymbolName {
        descriptor_index: u16,
        entry_index: u16,
        hint_name_rva: RelativeVirtualAddress,
    },
    NonAsciiSymbolName {
        descriptor_index: u16,
        entry_index: u16,
        hint_name_rva: RelativeVirtualAddress,
        offset: u32,
        byte: u8,
    },
    NameLengthLimitExceeded {
        descriptor_index: u16,
        entry_index: u16,
        hint_name_rva: RelativeVirtualAddress,
        limit: u32,
    },
    NameScanBudgetExceeded {
        descriptor_index: u16,
        entry_index: u16,
        hint_name_rva: RelativeVirtualAddress,
        offset: u32,
        limit: u32,
    },
}

fn named_symbol<'a>(
    prepared: &PreparedPe<'a>,
    descriptor_index: u16,
    entry_index: u16,
    hint_name_rva: RelativeVirtualAddress,
    total: &mut u32,
) -> Result<PeImportSymbol<'a>, PeImportLookupError> {
    let mut offset = 0;
    loop {
        if offset == NAME_LENGTH_LIMIT {
            return Err(PeImportLookupError::NameLengthLimitExceeded {
                descriptor_index,
                entry_index,
                hint_name_rva,
                limit: NAME_LENGTH_LIMIT,
            });
        }
        if *total == NAME_SCAN_BUDGET {
            return Err(PeImportLookupError::NameScanBudgetExceeded {
                descriptor_index,
                entry_index,
                hint_name_rva,
                offset,
                limit: NAME_SCAN_BUDGET,
            });
        }
        let prefix = prepared
            .resolve(hint_name_rva, 2 + offset + 1)
            .map_err(|cause| PeImportLookupError::HintNameRange {
                descriptor_index,
                entry_index,
                hint_name_rva,
                offset,
                cause,
            })?;
        let byte = prefix.bytes[2 + offset as usize];
        *total += 1;
        if byte == 0 {
            if offset == 0 {
                return Err(PeImportLookupError::EmptySymbolName {
                    descriptor_index,
                    entry_index,
                    hint_name_rva,
                });
            }
            let name = std::str::from_utf8(&prefix.bytes[2..2 + offset as usize])
                .expect("each preceding byte passed the ascii check");
            return Ok(PeImportSymbol::ByName {
                hint_name_rva,
                hint: read_u16(prefix.bytes, 0),
                name,
            });
        }
        if !byte.is_ascii() {
            return Err(PeImportLookupError::NonAsciiSymbolName {
                descriptor_index,
                entry_index,
                hint_name_rva,
                offset,
                byte,
            });
        }
        offset += 1;
    }
}

pub(super) fn lookup_entries<'a>(
    prepared: &PreparedPe<'a>,
    descriptor_index: u16,
    start: RelativeVirtualAddress,
    total_entries: &mut u32,
    total_name_bytes: &mut u32,
) -> Result<Vec<PeImportLookupEntry<'a>>, PeImportLookupError> {
    let kind = prepared.headers().prefix.kind;
    let (width, ordinal_flag, reserved_ordinal) = match kind {
        PeKind::Pe32 => (4, 1_u64 << 31, 0x7fff_0000),
        PeKind::Pe32Plus => (8, 1_u64 << 63, 0x7fff_ffff_ffff_0000),
    };
    let mut entries = Vec::new();
    for entry_index in 0..=ENTRY_LIMIT {
        let offset = u32::from(entry_index) * width;
        let length = offset + width;
        let prefix =
            prepared
                .resolve(start, length)
                .map_err(|cause| PeImportLookupError::LookupRange {
                    descriptor_index,
                    entry_index,
                    start,
                    length,
                    cause,
                })?;
        let record = &prefix.bytes[offset as usize..];
        let low = u64::from(read_u32(record, 0));
        let raw_value = match kind {
            PeKind::Pe32 => low,
            PeKind::Pe32Plus => low | (u64::from(read_u32(record, 4)) << 32),
        };
        if raw_value == 0 {
            return Ok(entries);
        }
        if entry_index == ENTRY_LIMIT {
            return Err(PeImportLookupError::EntryLimitExceeded {
                descriptor_index,
                entry_index,
                limit: ENTRY_LIMIT,
            });
        }
        if *total_entries == TOTAL_ENTRY_LIMIT {
            return Err(PeImportLookupError::TotalEntryLimitExceeded {
                descriptor_index,
                entry_index,
                limit: TOTAL_ENTRY_LIMIT,
            });
        }
        let symbol = if raw_value & ordinal_flag != 0 {
            if raw_value & reserved_ordinal != 0 {
                return Err(PeImportLookupError::InvalidOrdinalEncoding {
                    descriptor_index,
                    entry_index,
                    raw_value,
                    kind,
                });
            }
            PeImportSymbol::Ordinal(
                u16::try_from(raw_value & 0xffff).expect("masked to sixteen bits"),
            )
        } else {
            if raw_value & 0x7fff_ffff_8000_0000 != 0 {
                return Err(PeImportLookupError::InvalidNameEncoding {
                    descriptor_index,
                    entry_index,
                    raw_value,
                    kind,
                });
            }
            let hint_name_rva = RelativeVirtualAddress::new(
                u32::try_from(raw_value).expect("validated low31-bit name rva"),
            );
            named_symbol(
                prepared,
                descriptor_index,
                entry_index,
                hint_name_rva,
                total_name_bytes,
            )?
        };
        entries.push(PeImportLookupEntry {
            lookup_rva: RelativeVirtualAddress::new(start.get() + offset),
            lookup_file_offset: FileOffset::new(prefix.file_offset.get() + u64::from(offset)),
            raw_value,
            symbol,
        });
        *total_entries += 1;
    }
    unreachable!("the last allowed entry returns a terminator or a limit error")
}

/// validates all descriptors first, then reads only explicit lookup tables.
/// zero lookup coordinates never fall back to the iat. whole table and hint/name
/// prefixes use the conservative resolver. limits are 1024 entries per dll,
/// 4096 total entries, and 1024/65,536 symbol-name bytes including nul; hint bytes
/// and the separate dll-name budget do not consume the symbol-name budget.
///
/// # errors
/// returns typed descriptor, source, range, encoding and name failures without
/// partial results. fetched zero entries precede entry budgets; local budgets
/// precede global budgets, and name budgets precede hint/name reads.
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn parse_pe_import_lookups(
    bytes: &[u8],
) -> Result<Vec<PeImportLookup<'_>>, PeImportLookupError> {
    let prepared = PreparedPe::new(bytes)
        .map_err(|error| PeImportLookupError::Descriptors(PeImportError::Base(error)))?;
    let descriptors =
        parse_prepared_descriptors(&prepared).map_err(PeImportLookupError::Descriptors)?;
    let mut lookups = Vec::new();
    let mut total_entries = 0;
    let mut total_name_bytes = 0;
    for (descriptor_index, descriptor) in (0_u16..).zip(descriptors) {
        let start = descriptor.import_lookup_table_rva;
        if start.get() == 0 {
            return Err(PeImportLookupError::LookupTableUnavailable { descriptor_index });
        }
        let entries = lookup_entries(
            &prepared,
            descriptor_index,
            start,
            &mut total_entries,
            &mut total_name_bytes,
        )?;
        lookups.push(PeImportLookup {
            descriptor,
            entries,
        });
    }
    Ok(lookups)
}
