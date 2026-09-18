use super::{
    PeClrError, PeClrHeader, PeDirectoryAddress, PeHeaderError, PeHeaderPrefix, PeHeaders, PeKind,
    PeRvaError, parse_pe_header_prefix,
};
use crate::{FileOffset, RelativeVirtualAddress};

/// an owned value and its physical byte range within the inspected input.
/// offsets do not identify the source; widths are two or four bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeFieldEvidence<T> {
    pub value: T,
    pub file_offset: FileOffset,
    pub byte_length: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeHeaderPrefixEvidence {
    /// optional magic: `Pe32` represents 0x010b; `Pe32Plus` represents 0x020b.
    pub kind: PeFieldEvidence<PeKind>,
    pub machine: PeFieldEvidence<u16>,
    pub characteristics: PeFieldEvidence<u16>,
}

/// slot 14's raw declaration; the target bytes have not been validated.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeClrDescriptorEvidence {
    pub rva: PeFieldEvidence<RelativeVirtualAddress>,
    pub size: PeFieldEvidence<u32>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeOptionalHeaderEvidence {
    pub entry_rva: PeFieldEvidence<RelativeVirtualAddress>,
    pub subsystem: PeFieldEvidence<u16>,
    pub dll_characteristics: PeFieldEvidence<u16>,
    pub directory_count: PeFieldEvidence<u32>,
    /// absent only when slot 14 is undeclared; a declared zero pair is retained.
    pub clr_descriptor: Option<PeClrDescriptorEvidence>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeClrHeaderEvidence {
    pub flags: PeFieldEvidence<u32>,
    /// the raw word is retained without token or native-target interpretation.
    pub raw_entry_point: PeFieldEvidence<u32>,
}

/// independent reader outcomes; later errors preserve earlier successful fields.
/// at most eleven fields are retained, without an input or process memory cap.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PeDeclaredEvidence {
    pub prefix: Result<PeHeaderPrefixEvidence, PeHeaderError>,
    pub optional: Result<PeOptionalHeaderEvidence, PeHeaderError>,
    /// absence requires a valid base, including sections, under the clr reader.
    pub clr: Result<Option<PeClrHeaderEvidence>, PeClrError>,
}

fn field<T>(value: T, base: FileOffset, offset: u64, byte_length: u8) -> PeFieldEvidence<T> {
    PeFieldEvidence {
        value,
        file_offset: base
            .checked_add(offset)
            .expect("successful reader admitted this field's physical range"),
        byte_length,
    }
}

fn prefix_evidence(header: PeHeaderPrefix) -> PeHeaderPrefixEvidence {
    PeHeaderPrefixEvidence {
        kind: field(header.kind, header.pe_offset, 24, 2),
        machine: field(header.machine, header.pe_offset, 4, 2),
        characteristics: field(header.characteristics, header.pe_offset, 22, 2),
    }
}

fn optional_evidence(headers: &PeHeaders) -> PeOptionalHeaderEvidence {
    let optional_offset = headers
        .prefix
        .pe_offset
        .checked_add(24)
        .expect("successful header reader admitted the optional header");
    let fixed_size = match headers.prefix.kind {
        PeKind::Pe32 => 96,
        PeKind::Pe32Plus => 112,
    };
    let clr_descriptor = headers.directories[14].map(|directory| {
        let PeDirectoryAddress::Rva(rva) = directory.address else {
            unreachable!("the same-input header reader uses rva coordinates for slot 14");
        };
        PeClrDescriptorEvidence {
            rva: field(rva, optional_offset, fixed_size + 112, 4),
            size: field(directory.size, optional_offset, fixed_size + 116, 4),
        }
    });
    PeOptionalHeaderEvidence {
        entry_rva: field(
            headers.optional.address_of_entry_point,
            optional_offset,
            16,
            4,
        ),
        subsystem: field(headers.optional.subsystem, optional_offset, 68, 2),
        dll_characteristics: field(headers.optional.dll_characteristics, optional_offset, 70, 2),
        directory_count: field(
            headers.optional.number_of_rva_and_sizes,
            optional_offset,
            fixed_size - 4,
            4,
        ),
        clr_descriptor,
    }
}

fn clr_evidence(header: PeClrHeader) -> PeClrHeaderEvidence {
    PeClrHeaderEvidence {
        flags: field(header.flags, header.directory_file_offset, 16, 4),
        raw_entry_point: field(header.raw_entry_point, header.directory_file_offset, 20, 4),
    }
}

/// collects declared fields without a whole-image success or eligibility verdict.
///
/// preserves independent prefix, full-header and clr outcomes and validation depths.
/// same-call successful prefix and headers are reused; section and clr admission
/// still run before their results. a valid prefix can coexist with an optional-header error;
/// valid headers can coexist with a clr or section error. no recovery parsing occurs.
///
/// successful fields own raw values and physical input coordinates. machine,
/// subsystem, flags and entry words do not imply runtime requirements or support.
/// a declared zero clr descriptor remains visible even when the clr result is absent.
///
/// # panics
/// only if an internal successful-reader range or slot-kind invariant is violated.
/// malformed input is represented by the corresponding reader errors.
///
/// # example
/// ```
/// use ring3_core::{PeHeaderError, inspect_pe_declared_evidence};
/// let evidence = inspect_pe_declared_evidence(b"bad");
/// assert!(matches!(evidence.prefix, Err(PeHeaderError::InvalidDosSignature { .. })));
/// assert!(evidence.optional.is_err());
/// assert!(evidence.clr.is_err());
/// ```
pub fn inspect_pe_declared_evidence(bytes: &[u8]) -> PeDeclaredEvidence {
    let parsed_prefix = parse_pe_header_prefix(bytes);
    let headers =
        parsed_prefix.and_then(|prefix| super::optional::parse_after_prefix(bytes, prefix));
    let prefix = parsed_prefix.map(prefix_evidence);
    let optional = headers
        .as_ref()
        .map(optional_evidence)
        .map_err(|&cause| cause);
    let clr = headers
        .map_err(|cause| PeClrError::Base(PeRvaError::Parse(cause)))
        .and_then(|headers| {
            super::rva::PreparedPe::from_headers(bytes, &headers).map_err(PeClrError::Base)
        })
        .and_then(|prepared| super::clr::parse_prepared(&prepared))
        .map(|value| value.map(clr_evidence));
    PeDeclaredEvidence {
        prefix,
        optional,
        clr,
    }
}
