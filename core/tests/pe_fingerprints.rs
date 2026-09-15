use ring3_core::{
    FileOffset, PeClrError, PeFingerprintError, PeHeaderError, PeKind, PeRvaError,
    fingerprint_pe_declared_evidence,
};

fn digest(hex: &str) -> [u8; 32] {
    assert_eq!(hex.len(), 64);
    std::array::from_fn(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap())
}

fn prefix_fixture() -> Vec<u8> {
    let mut bytes = vec![0; 90];
    bytes[..2].copy_from_slice(b"MZ");
    bytes[0x3c..0x40].copy_from_slice(&64_u32.to_le_bytes());
    bytes[64..68].copy_from_slice(b"PE\0\0");
    bytes[68..70].copy_from_slice(&0xffff_u16.to_le_bytes());
    bytes[84..86].copy_from_slice(&2_u16.to_le_bytes());
    bytes[86..88].copy_from_slice(&0x1234_u16.to_le_bytes());
    bytes[88..90].copy_from_slice(&0x10b_u16.to_le_bytes());
    bytes
}

#[test]
fn hashes_known_vectors_even_when_the_input_is_not_a_pe_image() {
    for (bytes, expected) in [
        (
            b"".as_slice(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        ),
        (
            b"abc".as_slice(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        ),
        (
            b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq".as_slice(),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
        ),
    ] {
        let length = u64::try_from(bytes.len()).unwrap();
        let result = fingerprint_pe_declared_evidence(bytes, length).unwrap();
        assert_eq!(result.byte_length, length);
        assert_eq!(result.digest, digest(expected));
        let error = if bytes.is_empty() {
            PeHeaderError::OutOfBounds {
                offset: FileOffset::new(0),
                needed: 2,
                available: 0,
            }
        } else {
            PeHeaderError::InvalidDosSignature {
                offset: FileOffset::new(0),
            }
        };
        assert_eq!(result.evidence.prefix, Err(error));
        assert_eq!(result.evidence.optional, Err(error));
        assert_eq!(
            result.evidence.clr,
            Err(PeClrError::Base(PeRvaError::Parse(error)))
        );
    }
}

#[test]
fn refuses_an_over_budget_input_without_returning_partial_evidence() {
    for bytes in [b"".as_slice(), b"MZ".as_slice(), b"abc".as_slice()] {
        let length = u64::try_from(bytes.len()).unwrap();
        assert!(fingerprint_pe_declared_evidence(bytes, length).is_ok());
        assert!(fingerprint_pe_declared_evidence(bytes, u64::MAX).is_ok());
        if length != 0 {
            assert_eq!(
                fingerprint_pe_declared_evidence(bytes, length - 1),
                Err(PeFingerprintError::InputTooLarge {
                    length,
                    limit: length - 1
                }),
            );
        }
    }
}

#[test]
fn uninspected_and_trailing_bytes_change_the_hash_with_equal_metadata() {
    let bytes = prefix_fixture();
    let original = fingerprint_pe_declared_evidence(&bytes, 90).unwrap();
    let prefix = original.evidence.prefix.unwrap();
    assert_eq!(prefix.kind.value, PeKind::Pe32);
    assert_eq!(prefix.kind.file_offset, FileOffset::new(88));
    assert_eq!(prefix.machine.value, 0xffff);
    assert_eq!(prefix.characteristics.value, 0x1234);
    assert!(original.evidence.optional.is_err());
    assert!(original.evidence.clr.is_err());
    let mut stub = bytes.clone();
    stub[2] ^= 1;
    let mut trailing = bytes.clone();
    trailing.push(0);
    for changed in [stub, trailing] {
        let result = fingerprint_pe_declared_evidence(&changed, 91).unwrap();
        assert_eq!(result.evidence, original.evidence);
        assert_ne!(result.digest, original.digest);
        assert_eq!(result.byte_length, u64::try_from(changed.len()).unwrap());
    }
}

#[test]
fn results_own_the_fingerprint_and_evidence_after_input_is_overwritten() {
    let original = prefix_fixture();
    let mut input = original.clone();
    let first = fingerprint_pe_declared_evidence(&input, 90).unwrap();
    assert_eq!(input, original);
    assert_eq!(first, fingerprint_pe_declared_evidence(&input, 90).unwrap());
    input.fill(0);
    drop(input);
    assert_eq!(
        first,
        fingerprint_pe_declared_evidence(&original.clone(), 90).unwrap()
    );
    assert_eq!(first.byte_length, 90);
    assert_eq!(
        first.evidence.prefix.unwrap().characteristics.byte_length,
        2
    );
}

use ring3_core::{
    PeClrDescriptorEvidence, PeClrHeaderEvidence, PeDeclaredEvidence, PeFieldEvidence,
    PeFingerprintedEvidence, PeHeaderPrefixEvidence, PeOptionalHeaderEvidence,
    RelativeVirtualAddress,
};

fn compiled_field<T>(value: T, file_offset: u64, byte_length: u8) -> PeFieldEvidence<T> {
    PeFieldEvidence {
        value,
        file_offset: FileOffset::new(file_offset),
        byte_length,
    }
}

fn generated_fingerprint(variable: &str, expected: &PeFingerprintedEvidence) {
    let path = std::env::var(variable).expect("set the generated fixture path");
    let original = std::fs::read(&path).unwrap();
    assert_eq!(u64::try_from(original.len()).unwrap(), expected.byte_length);
    let mut bytes = original.clone();
    let actual = fingerprint_pe_declared_evidence(&bytes, expected.byte_length).unwrap();
    assert_eq!(&actual, expected);
    assert_eq!(
        fingerprint_pe_declared_evidence(&bytes, expected.byte_length).unwrap(),
        actual
    );
    assert_eq!(bytes, original);
    bytes.fill(0);
    drop(bytes);
    assert_eq!(
        fingerprint_pe_declared_evidence(&original, expected.byte_length).unwrap(),
        actual
    );
    assert_eq!(std::fs::read(path).unwrap(), original);
}

#[test]
#[ignore = "requires an explicitly generated fixture"]
fn generated_pe32_fingerprints_match_compiled_bytes_and_fields() {
    generated_fingerprint(
        "RING3_PE32_FIXTURE",
        &PeFingerprintedEvidence {
            byte_length: 1024,
            digest: digest("3e33f66a7f30b1cfc307e2591ad44f34148af17a7cad97f41bd8c8552ea698ac"),
            evidence: PeDeclaredEvidence {
                prefix: Ok(PeHeaderPrefixEvidence {
                    kind: compiled_field(PeKind::Pe32, 144, 2),
                    machine: compiled_field(332, 124, 2),
                    characteristics: compiled_field(259, 142, 2),
                }),
                optional: Ok(PeOptionalHeaderEvidence {
                    entry_rva: compiled_field(RelativeVirtualAddress::new(4096), 160, 4),
                    subsystem: compiled_field(3, 212, 2),
                    dll_characteristics: compiled_field(33024, 214, 2),
                    directory_count: compiled_field(16, 236, 4),
                    clr_descriptor: Some(PeClrDescriptorEvidence {
                        rva: compiled_field(RelativeVirtualAddress::new(0), 352, 4),
                        size: compiled_field(0, 356, 4),
                    }),
                }),
                clr: Ok(None),
            },
        },
    );
}

#[test]
#[ignore = "requires an explicitly generated fixture"]
fn generated_pe32plus_fingerprints_match_compiled_bytes_and_fields() {
    generated_fingerprint(
        "RING3_PE32PLUS_FIXTURE",
        &PeFingerprintedEvidence {
            byte_length: 1024,
            digest: digest("a07d0c1a4064c9b1e09682277cc2b57310f41a279cf663a9b3fa6b8a888a8e2c"),
            evidence: PeDeclaredEvidence {
                prefix: Ok(PeHeaderPrefixEvidence {
                    kind: compiled_field(PeKind::Pe32Plus, 144, 2),
                    machine: compiled_field(34404, 124, 2),
                    characteristics: compiled_field(35, 142, 2),
                }),
                optional: Ok(PeOptionalHeaderEvidence {
                    entry_rva: compiled_field(RelativeVirtualAddress::new(4096), 160, 4),
                    subsystem: compiled_field(3, 212, 2),
                    dll_characteristics: compiled_field(33056, 214, 2),
                    directory_count: compiled_field(16, 252, 4),
                    clr_descriptor: Some(PeClrDescriptorEvidence {
                        rva: compiled_field(RelativeVirtualAddress::new(0), 368, 4),
                        size: compiled_field(0, 372, 4),
                    }),
                }),
                clr: Ok(None),
            },
        },
    );
}

#[test]
#[ignore = "requires an explicitly generated fixture"]
fn generated_managed_pe32_fingerprints_match_compiled_bytes_and_fields() {
    generated_fingerprint(
        "RING3_CLR_PE32_FIXTURE",
        &PeFingerprintedEvidence {
            byte_length: 3584,
            digest: digest("4315faf842c9e8badf6a5872523472ca2a8df13e035a47e56a2ba5076912c92a"),
            evidence: PeDeclaredEvidence {
                prefix: Ok(PeHeaderPrefixEvidence {
                    kind: compiled_field(PeKind::Pe32, 152, 2),
                    machine: compiled_field(332, 132, 2),
                    characteristics: compiled_field(258, 150, 2),
                }),
                optional: Ok(PeOptionalHeaderEvidence {
                    entry_rva: compiled_field(RelativeVirtualAddress::new(9078), 168, 4),
                    subsystem: compiled_field(3, 220, 2),
                    dll_characteristics: compiled_field(34112, 222, 2),
                    directory_count: compiled_field(16, 244, 4),
                    clr_descriptor: Some(PeClrDescriptorEvidence {
                        rva: compiled_field(RelativeVirtualAddress::new(8200), 360, 4),
                        size: compiled_field(72, 364, 4),
                    }),
                }),
                clr: Ok(Some(PeClrHeaderEvidence {
                    flags: compiled_field(3, 536, 4),
                    raw_entry_point: compiled_field(0x0600_0001, 540, 4),
                })),
            },
        },
    );
}

#[test]
#[ignore = "requires an explicitly generated fixture"]
fn generated_managed_pe32plus_fingerprints_match_compiled_bytes_and_fields() {
    generated_fingerprint(
        "RING3_CLR_PE32PLUS_FIXTURE",
        &PeFingerprintedEvidence {
            byte_length: 3072,
            digest: digest("7d50f2f6db9b06867cd30e9426e96241a2e522f0704a26561cdd1525d60654a9"),
            evidence: PeDeclaredEvidence {
                prefix: Ok(PeHeaderPrefixEvidence {
                    kind: compiled_field(PeKind::Pe32Plus, 152, 2),
                    machine: compiled_field(34404, 132, 2),
                    characteristics: compiled_field(34, 150, 2),
                }),
                optional: Ok(PeOptionalHeaderEvidence {
                    entry_rva: compiled_field(RelativeVirtualAddress::new(0), 168, 4),
                    subsystem: compiled_field(3, 220, 2),
                    dll_characteristics: compiled_field(34112, 222, 2),
                    directory_count: compiled_field(16, 260, 4),
                    clr_descriptor: Some(PeClrDescriptorEvidence {
                        rva: compiled_field(RelativeVirtualAddress::new(8192), 376, 4),
                        size: compiled_field(72, 380, 4),
                    }),
                }),
                clr: Ok(Some(PeClrHeaderEvidence {
                    flags: compiled_field(1, 528, 4),
                    raw_entry_point: compiled_field(0x0600_0001, 532, 4),
                })),
            },
        },
    );
}
