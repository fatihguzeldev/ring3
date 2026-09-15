use ring3_core::{
    FileOffset, PeAmd64UnwindInfoV1, PeAmd64UnwindInfoV1Error, PeAmd64UnwindTailV1, PeHeaderError,
    PeKind, PeRvaError, RelativeVirtualAddress, parse_pe_amd64_unwind_info_v1,
};

#[test]
fn complete_base_validation_precedes_unwind_address_checks() {
    assert_eq!(
        parse_pe_amd64_unwind_info_v1(b"", RelativeVirtualAddress::new(1)),
        Err(PeAmd64UnwindInfoV1Error::Base(PeRvaError::Parse(
            PeHeaderError::OutOfBounds {
                offset: FileOffset::new(0),
                needed: 2,
                available: 0,
            }
        )))
    );
}

fn put32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn fixture() -> Vec<u8> {
    let mut bytes = vec![0; 2048];
    bytes[..2].copy_from_slice(b"MZ");
    put32(&mut bytes, 60, 128);
    bytes[128..132].copy_from_slice(b"PE\0\0");
    bytes[132..134].copy_from_slice(&0x8664_u16.to_le_bytes());
    bytes[134..136].copy_from_slice(&1_u16.to_le_bytes());
    bytes[148..150].copy_from_slice(&144_u16.to_le_bytes());
    bytes[152..154].copy_from_slice(&0x20b_u16.to_le_bytes());
    put32(&mut bytes, 212, 512);
    bytes[296..304].copy_from_slice(b"opaque\0\0");
    for (offset, value) in [(304, 1024), (308, 4096), (312, 1024), (316, 512)] {
        put32(&mut bytes, offset, value);
    }
    bytes
}

fn record(
    bytes: &mut [u8],
    offset: usize,
    header: [u8; 4],
    words: &[u16],
    padding: Option<u16>,
    tail: &[u32],
) {
    bytes[offset..offset + 4].copy_from_slice(&header);
    let mut cursor = offset + 4;
    for word in words.iter().copied().chain(padding) {
        bytes[cursor..cursor + 2].copy_from_slice(&word.to_le_bytes());
        cursor += 2;
    }
    for word in tail {
        put32(bytes, cursor, *word);
        cursor += 4;
    }
}

fn rva(value: u32) -> RelativeVirtualAddress {
    RelativeVirtualAddress::new(value)
}

#[test]
fn raw_words_padding_and_scalars_outlive_changed_inputs() {
    let result = {
        let mut bytes = fixture();
        record(
            &mut bytes,
            512,
            [9, 7, 1, 0xf3],
            &[0x7b12],
            Some(0xa55a),
            &[0x1234_5678],
        );
        let before = bytes.clone();
        let result = parse_pe_amd64_unwind_info_v1(&bytes, rva(4096)).unwrap();
        assert_eq!(
            parse_pe_amd64_unwind_info_v1(&bytes, rva(4096)),
            Ok(result.clone())
        );
        assert_eq!(bytes, before);
        bytes.fill(0);
        result
    };
    assert_eq!(
        result,
        PeAmd64UnwindInfoV1 {
            rva: rva(4096),
            file_offset: FileOffset::new(512),
            byte_length: 12,
            version: 1,
            flags: 1,
            prolog_size: 7,
            code_count: 1,
            frame_register: 3,
            frame_offset_scaled: 15,
            code_words: vec![0x7b12],
            padding_word: Some(0xa55a),
            tail: PeAmd64UnwindTailV1::Handler {
                handler_rva: rva(0x1234_5678)
            },
        }
    );
}

#[test]
fn fixed_tail_tags_preserve_zero_and_unmapped_targets() {
    for flags in [1, 2, 3] {
        let mut bytes = fixture();
        record(
            &mut bytes,
            512,
            [1 | (flags << 3), 255, 0, 0xf0],
            &[],
            None,
            &[0],
        );
        let info = parse_pe_amd64_unwind_info_v1(&bytes, rva(4096)).unwrap();
        assert_eq!(info.flags, flags);
        assert_eq!(info.byte_length, 8);
        assert_eq!(info.frame_register, 0);
        assert_eq!(info.frame_offset_scaled, 15);
        assert_eq!(
            info.tail,
            PeAmd64UnwindTailV1::Handler {
                handler_rva: rva(0)
            }
        );
    }
    let mut bytes = fixture();
    record(
        &mut bytes,
        512,
        [33, 0, 2, 0],
        &[0xffff, 0],
        None,
        &[u32::MAX, 0, 0xffff_fffd],
    );
    let info = parse_pe_amd64_unwind_info_v1(&bytes, rva(4096)).unwrap();
    assert_eq!(info.byte_length, 20);
    assert_eq!(info.code_words, [0xffff, 0]);
    assert_eq!(info.padding_word, None);
    assert_eq!(
        info.tail,
        PeAmd64UnwindTailV1::Chain {
            begin_rva: rva(u32::MAX),
            end_rva: rva(0),
            unwind_info_rva: rva(0xffff_fffd)
        }
    );
}

#[test]
fn zero_and_maximum_slot_counts_keep_padding_separate() {
    let mut bytes = fixture();
    record(&mut bytes, 512, [1, 0, 0, 0], &[], None, &[]);
    let empty = parse_pe_amd64_unwind_info_v1(&bytes, rva(4096)).unwrap();
    assert_eq!(empty.byte_length, 4);
    assert_eq!(empty.code_count, 0);
    assert!(empty.code_words.is_empty());
    assert_eq!(empty.padding_word, None);
    assert_eq!(empty.tail, PeAmd64UnwindTailV1::None);
    let words: Vec<u16> = (0..255).collect();
    record(
        &mut bytes,
        512,
        [33, 255, 255, 0xff],
        &words,
        Some(0xabcd),
        &[0, 0, 0],
    );
    let max = parse_pe_amd64_unwind_info_v1(&bytes, rva(4096)).unwrap();
    assert_eq!(max.byte_length, 528);
    assert_eq!(max.code_count, 255);
    assert_eq!(max.code_words, words);
    assert_eq!(max.padding_word, Some(0xabcd));
    assert_eq!(
        max.tail,
        PeAmd64UnwindTailV1::Chain {
            begin_rva: rva(0),
            end_rva: rva(0),
            unwind_info_rva: rva(0)
        }
    );
}

#[test]
fn image_alignment_version_and_flags_have_stable_priority() {
    let mut bytes = fixture();
    bytes[132..134].copy_from_slice(&0xaa64_u16.to_le_bytes());
    assert_eq!(
        parse_pe_amd64_unwind_info_v1(&bytes, rva(4097)),
        Err(PeAmd64UnwindInfoV1Error::UnsupportedImage {
            machine: 0xaa64,
            kind: PeKind::Pe32Plus
        })
    );
    bytes[132..134].copy_from_slice(&0x8664_u16.to_le_bytes());
    assert_eq!(
        parse_pe_amd64_unwind_info_v1(&bytes, rva(u32::MAX)),
        Err(PeAmd64UnwindInfoV1Error::Unaligned { rva: rva(u32::MAX) })
    );
    put32(&mut bytes, 304, 4);
    put32(&mut bytes, 312, 4);
    bytes[512..516].copy_from_slice(&[0xff, 255, 255, 255]);
    assert_eq!(
        parse_pe_amd64_unwind_info_v1(&bytes, rva(4096)),
        Err(PeAmd64UnwindInfoV1Error::UnsupportedVersion {
            rva: rva(4096),
            version: 7
        })
    );
    bytes[512] = 0xf9;
    assert_eq!(
        parse_pe_amd64_unwind_info_v1(&bytes, rva(4096)),
        Err(PeAmd64UnwindInfoV1Error::UnsupportedFlags {
            rva: rva(4096),
            flags: 31
        })
    );
    bytes[512] = 41;
    assert_eq!(
        parse_pe_amd64_unwind_info_v1(&bytes, rva(4096)),
        Err(PeAmd64UnwindInfoV1Error::ConflictingFlags {
            rva: rva(4096),
            flags: 5
        })
    );
}

#[test]
fn header_and_whole_record_backing_fail_in_distinct_stages() {
    let mut bytes = fixture();
    record(&mut bytes, 512, [1, 0, 1, 0], &[0xabcd], Some(0), &[]);
    put32(&mut bytes, 304, 2);
    put32(&mut bytes, 312, 2);
    assert_eq!(
        parse_pe_amd64_unwind_info_v1(&bytes, rva(4096)),
        Err(PeAmd64UnwindInfoV1Error::HeaderRange {
            rva: rva(4096),
            cause: PeRvaError::CrossesRegionBoundary {
                start: rva(4096),
                length: 4
            }
        })
    );
    put32(&mut bytes, 304, 6);
    put32(&mut bytes, 312, 6);
    assert_eq!(
        parse_pe_amd64_unwind_info_v1(&bytes, rva(4096)),
        Err(PeAmd64UnwindInfoV1Error::RecordRange {
            rva: rva(4096),
            length: 8,
            cause: PeRvaError::CrossesRegionBoundary {
                start: rva(4096),
                length: 8
            }
        })
    );
    put32(&mut bytes, 304, 1024);
    assert_eq!(
        parse_pe_amd64_unwind_info_v1(&bytes, rva(4096)),
        Err(PeAmd64UnwindInfoV1Error::RecordRange {
            rva: rva(4096),
            length: 8,
            cause: PeRvaError::NotFileBacked {
                start: rva(4096),
                length: 8,
                section_index: 0
            }
        })
    );
    put32(&mut bytes, 304, 6);
    put32(&mut bytes, 312, 1024);
    assert_eq!(
        parse_pe_amd64_unwind_info_v1(&bytes, rva(4096)),
        Err(PeAmd64UnwindInfoV1Error::RecordRange {
            rva: rva(4096),
            length: 8,
            cause: PeRvaError::RawPaddingUnsupported {
                start: rva(4096),
                length: 8,
                section_index: 0
            }
        })
    );
}

#[test]
fn header_backing_and_odd_physical_alignment_are_preserved() {
    for (address, file) in [(400, 400), (4096, 513)] {
        let mut bytes = fixture();
        put32(&mut bytes, 316, 513);
        record(&mut bytes, file, [1, 0, 1, 0], &[0x1234], Some(0x5678), &[]);
        let info = parse_pe_amd64_unwind_info_v1(&bytes, rva(address)).unwrap();
        assert_eq!(info.rva, rva(address));
        assert_eq!(info.file_offset, FileOffset::new(file as u64));
        assert_eq!(info.code_words, [0x1234]);
        assert_eq!(info.padding_word, Some(0x5678));
    }
}

#[test]
fn exact_coordinate_end_is_allowed_and_a_larger_record_overflows() {
    let mut bytes = fixture();
    put32(&mut bytes, 304, 4);
    put32(&mut bytes, 308, 0xffff_fffc);
    put32(&mut bytes, 312, 4);
    record(&mut bytes, 512, [1, 0, 0, 0], &[], None, &[]);
    assert_eq!(
        parse_pe_amd64_unwind_info_v1(&bytes, rva(0xffff_fffc))
            .unwrap()
            .byte_length,
        4
    );
    bytes[514] = 1;
    assert_eq!(
        parse_pe_amd64_unwind_info_v1(&bytes, rva(0xffff_fffc)),
        Err(PeAmd64UnwindInfoV1Error::RecordRange {
            rva: rva(0xffff_fffc),
            length: 8,
            cause: PeRvaError::RvaRangeOverflow {
                start: rva(0xffff_fffc),
                length: 8
            }
        })
    );
}

#[test]
fn explicit_rva_does_not_require_exception_directory_membership() {
    let mut bytes = fixture();
    record(&mut bytes, 512, [1, 0, 0, 0], &[], None, &[]);
    assert!(parse_pe_amd64_unwind_info_v1(&bytes, rva(4096)).is_ok());
    put32(&mut bytes, 260, 4);
    put32(&mut bytes, 288, u32::MAX);
    put32(&mut bytes, 292, 1);
    assert!(parse_pe_amd64_unwind_info_v1(&bytes, rva(4096)).is_ok());
    assert_eq!(
        parse_pe_amd64_unwind_info_v1(&bytes, rva(0)),
        Err(PeAmd64UnwindInfoV1Error::UnsupportedVersion {
            rva: rva(0),
            version: 5
        })
    );
}
