use super::super::rva::PreparedPe;
use super::super::{PeKind, PeRvaError};
use crate::{FileOffset, RelativeVirtualAddress};

/// fixed raw tail coordinates; targets and language-specific bytes remain unread.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeAmd64UnwindTailV1 {
    None,
    Handler {
        handler_rva: RelativeVirtualAddress,
    },
    Chain {
        begin_rva: RelativeVirtualAddress,
        end_rva: RelativeVirtualAddress,
        unwind_info_rva: RelativeVirtualAddress,
    },
}

/// owned version1 framing metadata, not a validated unwind program.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PeAmd64UnwindInfoV1 {
    pub rva: RelativeVirtualAddress,
    pub file_offset: FileOffset,
    pub byte_length: u32,
    pub version: u8,
    pub flags: u8,
    pub prolog_size: u8,
    pub code_count: u8,
    pub frame_register: u8,
    /// raw high nibble; not multiplied by sixteen.
    pub frame_offset_scaled: u8,
    /// exactly `code_count` raw slots; odd alignment padding is separate.
    pub code_words: Vec<u16>,
    pub padding_word: Option<u16>,
    pub tail: PeAmd64UnwindTailV1,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PeAmd64UnwindInfoV1Error {
    Base(PeRvaError),
    UnsupportedImage {
        machine: u16,
        kind: PeKind,
    },
    Unaligned {
        rva: RelativeVirtualAddress,
    },
    HeaderRange {
        rva: RelativeVirtualAddress,
        cause: PeRvaError,
    },
    UnsupportedVersion {
        rva: RelativeVirtualAddress,
        version: u8,
    },
    UnsupportedFlags {
        rva: RelativeVirtualAddress,
        flags: u8,
    },
    ConflictingFlags {
        rva: RelativeVirtualAddress,
        flags: u8,
    },
    RecordRange {
        rva: RelativeVirtualAddress,
        length: u32,
        cause: PeRvaError,
    },
}

/// reads one explicitly addressed version1 amd64 unwind metadata envelope.
///
/// requires amd64 pe32+ and a four-byte aligned rva, but no physical alignment
/// or exception-directory membership. rva zero is an explicit request. raw code
/// slots, padding, frame/prolog fields and fixed tail targets are preserved;
/// operations, handler data and chain targets are not interpreted or followed.
/// flag values 0, 1, 2, 3 and 4 are structurally admitted, not semantically validated.
///
/// the declared u8 slot count bounds the envelope to 528 bytes including padding
/// and the largest fixed tail. this is not an input, acquisition or memory cap.
/// a successful result owns all data and does not establish unwind correctness
/// or image loadability. the existing function-table reader remains independent.
///
/// # errors
/// validates the complete base image, machine/kind, rva alignment, four-byte
/// header backing, version, unknown flags, conflicting flags and whole-record
/// backing, in that order. errors retain their range context and no partial data.
///
/// ```
/// use ring3_core::{PeAmd64UnwindInfoV1Error, RelativeVirtualAddress,
///     parse_pe_amd64_unwind_info_v1};
/// assert!(matches!(parse_pe_amd64_unwind_info_v1(b"invalid", RelativeVirtualAddress::new(4096)),
///     Err(PeAmd64UnwindInfoV1Error::Base(_))));
/// ```
#[expect(
    clippy::missing_errors_doc,
    reason = "project documentation headings are lower case"
)]
pub fn parse_pe_amd64_unwind_info_v1(
    bytes: &[u8],
    rva: RelativeVirtualAddress,
) -> Result<PeAmd64UnwindInfoV1, PeAmd64UnwindInfoV1Error> {
    let prepared = PreparedPe::new(bytes).map_err(PeAmd64UnwindInfoV1Error::Base)?;
    let prefix = prepared.headers().prefix;
    if prefix.machine != 0x8664 || prefix.kind != PeKind::Pe32Plus {
        return Err(PeAmd64UnwindInfoV1Error::UnsupportedImage {
            machine: prefix.machine,
            kind: prefix.kind,
        });
    }
    if !rva.get().is_multiple_of(4) {
        return Err(PeAmd64UnwindInfoV1Error::Unaligned { rva });
    }
    let header = prepared
        .resolve(rva, 4)
        .map_err(|cause| PeAmd64UnwindInfoV1Error::HeaderRange { rva, cause })?;
    let version = header.bytes[0] & 7;
    let flags = header.bytes[0] >> 3;
    if version != 1 {
        return Err(PeAmd64UnwindInfoV1Error::UnsupportedVersion { rva, version });
    }
    if flags & !7 != 0 {
        return Err(PeAmd64UnwindInfoV1Error::UnsupportedFlags { rva, flags });
    }
    if flags & 4 != 0 && flags & 3 != 0 {
        return Err(PeAmd64UnwindInfoV1Error::ConflictingFlags { rva, flags });
    }
    let code_count = header.bytes[2];
    let padded_slots = (u32::from(code_count) + 1) & !1;
    let tail_offset = 4 + padded_slots * 2;
    let tail_bytes = if flags & 4 != 0 {
        12
    } else if flags & 3 != 0 {
        4
    } else {
        0
    };
    let byte_length = tail_offset + tail_bytes;
    let range = prepared.resolve(rva, byte_length).map_err(|cause| {
        PeAmd64UnwindInfoV1Error::RecordRange {
            rva,
            length: byte_length,
            cause,
        }
    })?;
    let raw = range.bytes;
    let code_end = 4 + usize::from(code_count) * 2;
    let code_words = raw[4..code_end]
        .chunks_exact(2)
        .map(|b| u16::from_le_bytes([b[0], b[1]]))
        .collect();
    let padding_word = if code_count % 2 == 1 {
        Some(u16::from_le_bytes([raw[code_end], raw[code_end + 1]]))
    } else {
        None
    };
    let tail_offset = tail_offset as usize;
    let word = |offset| RelativeVirtualAddress::new(super::super::optional::read_u32(raw, offset));
    let tail = if flags & 4 != 0 {
        PeAmd64UnwindTailV1::Chain {
            begin_rva: word(tail_offset),
            end_rva: word(tail_offset + 4),
            unwind_info_rva: word(tail_offset + 8),
        }
    } else if flags & 3 != 0 {
        PeAmd64UnwindTailV1::Handler {
            handler_rva: word(tail_offset),
        }
    } else {
        PeAmd64UnwindTailV1::None
    };
    Ok(PeAmd64UnwindInfoV1 {
        rva,
        file_offset: range.file_offset,
        byte_length,
        version,
        flags,
        prolog_size: raw[1],
        code_count,
        frame_register: raw[3] & 15,
        frame_offset_scaled: raw[3] >> 4,
        code_words,
        padding_word,
        tail,
    })
}
