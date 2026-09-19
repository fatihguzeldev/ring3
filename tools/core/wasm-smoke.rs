use ring3_core::{
    AsciiSourcePathBatch, AsciiSourcePathEntry, AsciiSourcePathError, AsciiSourcePathLimits,
    AsciiSourcePathSegmentError, FileOffset, PeFileRange, PeFileRangeSource, PeFingerprintError,
    PeHeaderPrefix, PeKind, PeRvaError, RelativeVirtualAddress, admit_ascii_source_paths,
    fingerprint_pe_declared_evidence, parse_pe_headers, resolve_pe_file_range,
};

fn fixture() -> [u8; 528] {
    let mut bytes = [0; 528];
    bytes[..2].copy_from_slice(b"MZ");
    bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
    for (offset, value) in [
        (0x84, 0x8664_u16),
        (0x86, 1),
        (0x94, 240),
        (0x96, 0x22),
        (0x98, 0x20b),
        (0xdc, 3),
    ] {
        bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }
    for (offset, value) in [
        (0x3c, 0x80_u32),
        (0xa8, 0x1000),
        (0xac, 0x1000),
        (0xb8, 0x1000),
        (0xbc, 0x200),
        (0xd0, 0x2000),
        (0xd4, 0x200),
        (0x104, 16),
        (0x190, 16),
        (0x194, 0x1000),
        (0x198, 16),
        (0x19c, 0x200),
        (0x1ac, 0x4000_0040),
    ] {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes[0xb0..0xb8].copy_from_slice(&0x1_4000_0000_u64.to_le_bytes());
    bytes[0x188..0x18d].copy_from_slice(b".test");
    for (slot, value) in bytes[512..].iter_mut().zip(0x10_u8..0x20) {
        *slot = value;
    }
    bytes
}

fn inspect_image() {
    let mut bytes = fixture();
    let original = bytes;
    let headers = parse_pe_headers(&bytes).unwrap();
    assert_eq!(
        headers.prefix,
        PeHeaderPrefix {
            pe_offset: FileOffset::new(0x80),
            machine: 0x8664,
            number_of_sections: 1,
            characteristics: 0x22,
            size_of_optional_header: 240,
            kind: PeKind::Pe32Plus,
        }
    );
    assert_eq!(headers.optional.image_base, 0x1_4000_0000);
    assert_eq!(
        resolve_pe_file_range(&bytes, RelativeVirtualAddress::new(0x1003), 4),
        Ok(PeFileRange {
            file_offset: FileOffset::new(0x203),
            bytes: &[0x13, 0x14, 0x15, 0x16],
            source: PeFileRangeSource::Section(0),
        })
    );
    assert_eq!(
        resolve_pe_file_range(&bytes, RelativeVirtualAddress::new(0x100f), 2),
        Err(PeRvaError::CrossesRegionBoundary {
            start: RelativeVirtualAddress::new(0x100f),
            length: 2,
        })
    );
    assert_eq!(
        fingerprint_pe_declared_evidence(&bytes, 527),
        Err(PeFingerprintError::InputTooLarge {
            length: 528,
            limit: 527,
        })
    );
    let fingerprint = fingerprint_pe_declared_evidence(&bytes, 528).unwrap();
    assert_eq!(
        fingerprint_pe_declared_evidence(&bytes, 528),
        Ok(fingerprint)
    );
    assert_eq!(bytes, original);
    bytes.fill(0);
    assert_eq!(fingerprint.byte_length, 528);
    assert_eq!(
        fingerprint.digest,
        [
            0x56, 0x28, 0x84, 0x9c, 0x7d, 0x69, 0xb0, 0xec, 0xcb, 0x42, 0x67, 0x03, 0xc5, 0x75,
            0x5e, 0xdd, 0xf1, 0x46, 0x97, 0x09, 0x3b, 0x20, 0x91, 0xa4, 0x2c, 0x79, 0x17, 0x07,
            0xc2, 0xe7, 0xd5, 0xd1,
        ]
    );
}

fn inspect_paths() {
    let limits = AsciiSourcePathLimits {
        max_paths: 1,
        max_path_bytes: 12,
        max_total_path_bytes: 12,
        max_depth: 2,
    };
    let paths = {
        let input = String::from(r"Bin\GAME.exe");
        admit_ascii_source_paths(&[&input], limits).unwrap()
    };
    assert_eq!(
        paths,
        AsciiSourcePathBatch {
            total_path_bytes: 12,
            entries: vec![AsciiSourcePathEntry {
                index: 0,
                normalized: "Bin/GAME.exe".into(),
                key: "bin/game.exe".into(),
                depth: 2,
            }],
        }
    );
    assert_eq!(
        admit_ascii_source_paths(&["../GAME.exe"], limits),
        Err(AsciiSourcePathError::Segment {
            index: 0,
            segment: 0,
            reason: AsciiSourcePathSegmentError::Parent,
        })
    );
}

// this isolated test cdylib owns its unique zero-argument export.
#[unsafe(no_mangle)]
pub extern "C" fn run() -> u32 {
    inspect_image();
    inspect_paths();
    0x5233_0001
}
