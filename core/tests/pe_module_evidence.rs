use ring3_core::{
    PeDelayImportEvidenceError, PeDelayImportEvidenceLimits, PeExportEvidenceError,
    PeExportEvidenceLimits, PeFingerprintError, PeModuleEvidence, PeModuleEvidenceLimits,
    PeModuleOutputLimits, PeStaticImportEvidenceError, PeStaticImportEvidenceLimits,
    fingerprint_pe_declared_evidence, inspect_pe_delay_imports, inspect_pe_exports,
    inspect_pe_module_evidence, inspect_pe_static_imports,
};

fn put(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
fn word(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}
fn offset(rva: usize) -> usize {
    rva - 0x1000 + 512
}
fn text(bytes: &mut [u8], rva: usize, value: &[u8]) {
    bytes[offset(rva)..offset(rva) + value.len()].copy_from_slice(value);
}
fn slot(plus: bool) -> usize {
    152 + if plus { 112 } else { 96 }
}
fn fixture(plus: bool) -> Vec<u8> {
    let mut b = vec![0; 8192];
    b[..2].copy_from_slice(b"MZ");
    put(&mut b, 60, 128);
    b[128..132].copy_from_slice(b"PE\0\0");
    word(&mut b, 132, if plus { 0x8664 } else { 0x14c });
    word(&mut b, 134, 1);
    word(&mut b, 150, 0x2002);
    word(&mut b, 148, if plus { 240 } else { 224 });
    word(&mut b, 152, if plus { 0x20b } else { 0x10b });
    put(&mut b, 168, 0x3300);
    word(&mut b, 220, 3);
    word(&mut b, 222, 0x140);
    put(&mut b, 212, 512);
    put(&mut b, slot(plus) - 4, 16);
    for (i, rva, size) in [(0, 0x1000, 0x200), (1, 0x2000, 40), (13, 0x2800, 64)] {
        put(&mut b, slot(plus) + i * 8, rva);
        put(&mut b, slot(plus) + i * 8 + 4, size);
    }
    for (o, v) in [(8, 7680), (12, 0x1000), (16, 7680), (20, 512)] {
        put(&mut b, slot(plus) + 128 + o, v);
    }
    for (o, v) in [
        (0, 0x1234_5678),
        (4, 0xabcd_ef12),
        (12, u32::MAX),
        (16, 65534),
        (20, 3),
        (24, 2),
        (28, 0x1300),
        (32, 0x1320),
        (36, 0x1340),
    ] {
        put(&mut b, offset(0x1000) + o, v);
    }
    word(&mut b, offset(0x1000) + 8, 2);
    word(&mut b, offset(0x1000) + 10, 9);
    text(&mut b, 0x1100, b"Other.#32768\0");
    for (i, v) in [0x3300, 0, 0x1100].into_iter().enumerate() {
        put(&mut b, offset(0x1300) + i * 4, v);
    }
    for (i, (rva, index, name)) in [(0x1360, 2, b"Alias\0".as_slice()), (0x1380, 0, b"Direct\0")]
        .into_iter()
        .enumerate()
    {
        put(&mut b, offset(0x1320) + i * 4, u32::try_from(rva).unwrap());
        word(&mut b, offset(0x1340) + i * 2, index);
        text(&mut b, rva, name);
    }
    for (i, v) in [0x2200, 0x8765_4321, u32::MAX, 0x2100, 0xfeed_beef]
        .into_iter()
        .enumerate()
    {
        put(&mut b, offset(0x2000) + i * 4, v);
    }
    text(&mut b, 0x2100, b"Static.DLL\0");
    word(&mut b, offset(0x2300), 0xabcd);
    text(&mut b, 0x2302, b"Symbol\0");
    for (i, v) in [
        1,
        0x2900,
        u32::MAX,
        0xdead_beef,
        0x2a00,
        0x1234_5678,
        7,
        0x9876_5432,
    ]
    .into_iter()
    .enumerate()
    {
        put(&mut b, offset(0x2800) + i * 4, v);
    }
    text(&mut b, 0x2900, b"Delay.DLL\0");
    word(&mut b, offset(0x2b00), 0x4321);
    text(&mut b, 0x2b02, b"Later\0");
    let width = if plus { 8 } else { 4 };
    let flag = if plus { 1_u64 << 63 } else { 1 << 31 };
    for (rva, values) in [
        (0x2200, vec![0x2300, flag | 65535, 0x2300]),
        (0x2a00, vec![flag | 32768, 0x2b00]),
    ] {
        for (i, v) in values.into_iter().enumerate() {
            let o = offset(rva) + i * width;
            b[o..o + width].copy_from_slice(&v.to_le_bytes()[..width]);
        }
    }
    b
}
fn output(rows: u64, text_bytes: u64) -> PeModuleOutputLimits {
    PeModuleOutputLimits {
        max_rows: rows,
        max_text_bytes: text_bytes,
    }
}
fn limits() -> PeModuleEvidenceLimits {
    PeModuleEvidenceLimits {
        max_input_bytes: 8192,
        static_imports: output(5, 32),
        delay_imports: output(5, 23),
        exports: output(8, 35),
    }
}
fn individual(bytes: &[u8], l: PeModuleEvidenceLimits) -> PeModuleEvidence {
    PeModuleEvidence {
        fingerprinted: fingerprint_pe_declared_evidence(bytes, l.max_input_bytes).unwrap(),
        static_imports: inspect_pe_static_imports(
            bytes,
            PeStaticImportEvidenceLimits {
                max_input_bytes: l.max_input_bytes,
                max_output_rows: l.static_imports.max_rows,
                max_output_text_bytes: l.static_imports.max_text_bytes,
            },
        ),
        delay_imports: inspect_pe_delay_imports(
            bytes,
            PeDelayImportEvidenceLimits {
                max_input_bytes: l.max_input_bytes,
                max_output_rows: l.delay_imports.max_rows,
                max_output_text_bytes: l.delay_imports.max_text_bytes,
            },
        ),
        exports: inspect_pe_exports(
            bytes,
            PeExportEvidenceLimits {
                max_input_bytes: l.max_input_bytes,
                max_output_rows: l.exports.max_rows,
                max_output_text_bytes: l.exports.max_text_bytes,
            },
        ),
    }
}

#[test]
fn both_widths_own_complete_same_input_results_after_input_release() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        let original = bytes.clone();
        let expected = individual(&bytes, limits());
        let evidence = inspect_pe_module_evidence(&bytes, limits()).unwrap();
        assert_eq!(evidence, expected);
        assert_eq!(
            evidence,
            inspect_pe_module_evidence(&bytes, limits()).unwrap()
        );
        assert_eq!(bytes, original);
        bytes.fill(0xee);
        drop(bytes);
        assert_eq!(evidence.clone(), expected);
        let s = evidence.static_imports.unwrap();
        let d = evidence.delay_imports.unwrap();
        let e = evidence.exports.unwrap();
        assert_eq!((s.total_rows, s.total_text_bytes), (5, 32));
        assert_eq!((d.total_rows, d.total_text_bytes), (5, 23));
        assert_eq!((e.total_rows, e.total_text_bytes), (8, 35));
        assert!(s.descriptors.is_ok() && s.lookups.is_ok());
        assert!(d.descriptors.is_ok() && d.names.is_ok() && d.lookups.is_ok());
        assert!(e.directory.is_ok() && e.addresses.is_ok() && e.names.is_ok());
    }
}

#[test]
fn uninspected_tail_changes_digest_while_all_metadata_stays_equal() {
    for plus in [false, true] {
        let mut bytes = fixture(plus);
        let before = inspect_pe_module_evidence(&bytes, limits()).unwrap();
        bytes[8191] = 0x91;
        let after = inspect_pe_module_evidence(&bytes, limits()).unwrap();
        assert_ne!(before.fingerprinted.digest, after.fingerprinted.digest);
        assert_eq!(
            before.fingerprinted.byte_length,
            after.fingerprinted.byte_length
        );
        assert_eq!(before.fingerprinted.evidence, after.fingerprinted.evidence);
        assert_eq!(before.static_imports, after.static_imports);
        assert_eq!(before.delay_imports, after.delay_imports);
        assert_eq!(before.exports, after.exports);
    }
}

#[test]
fn shared_input_refusal_precedes_malformed_metadata_and_family_caps() {
    for bytes in [fixture(false), fixture(true), vec![0; 64]] {
        let length = u64::try_from(bytes.len()).unwrap();
        let l = PeModuleEvidenceLimits {
            max_input_bytes: length - 1,
            static_imports: output(0, 0),
            delay_imports: output(0, 0),
            exports: output(0, 0),
        };
        assert_eq!(
            inspect_pe_module_evidence(&bytes, l),
            Err(PeFingerprintError::InputTooLarge {
                length,
                limit: length - 1
            })
        );
    }
}

#[test]
fn reader_errors_preserve_other_families_and_complete_prior_views() {
    for plus in [false, true] {
        let original = inspect_pe_module_evidence(&fixture(plus), limits()).unwrap();
        for variant in 0..6 {
            let mut b = fixture(plus);
            match variant {
                0 => put(&mut b, offset(0x2000), 0),
                1 => put(&mut b, offset(0x2800) + 16, 0),
                2 => put(&mut b, offset(0x2800), 0),
                3 => word(&mut b, offset(0x1340), 3),
                4 => put(&mut b, offset(0x1000) + 28, 0),
                _ => {
                    put(&mut b, offset(0x2000), 0);
                    put(&mut b, offset(0x2800) + 16, 0);
                    word(&mut b, offset(0x1340), 3);
                }
            }
            let e = inspect_pe_module_evidence(&b, limits()).unwrap();
            assert_eq!(e, individual(&b, limits()));
            if variant == 0 || variant == 5 {
                let s = e.static_imports.as_ref().unwrap();
                assert!(s.descriptors.is_ok() && s.lookups.is_err());
            } else {
                assert_eq!(e.static_imports, original.static_imports);
            }
            if variant == 1 || variant == 2 || variant == 5 {
                let d = e.delay_imports.as_ref().unwrap();
                assert!(d.descriptors.is_ok() && d.lookups.is_err());
                assert_eq!(d.names.is_err(), variant == 2);
            } else {
                assert_eq!(e.delay_imports, original.delay_imports);
            }
            if variant >= 3 {
                let x = e.exports.unwrap();
                assert!(x.directory.is_ok() && x.names.is_err());
                assert_eq!(x.addresses.is_err(), variant == 4);
            } else {
                assert_eq!(e.exports, original.exports);
            }
        }
    }
}

#[test]
fn exact_and_one_less_family_caps_preserve_totals_and_independent_results() {
    for plus in [false, true] {
        let b = fixture(plus);
        for family in 0..3 {
            for rows_first in [true, false] {
                let mut l = limits();
                let cap = match family {
                    0 => &mut l.static_imports,
                    1 => &mut l.delay_imports,
                    _ => &mut l.exports,
                };
                if rows_first {
                    cap.max_rows -= 1;
                    cap.max_text_bytes = 0;
                } else {
                    cap.max_text_bytes -= 1;
                }
                let e = inspect_pe_module_evidence(&b, l).unwrap();
                assert_eq!(e, individual(&b, l));
                match family {
                    0 => {
                        assert_eq!(
                            e.static_imports,
                            Err(if rows_first {
                                PeStaticImportEvidenceError::OutputRowsExceeded {
                                    rows: 5,
                                    limit: 4,
                                }
                            } else {
                                PeStaticImportEvidenceError::OutputTextExceeded {
                                    bytes: 32,
                                    limit: 31,
                                }
                            })
                        );
                        assert!(e.delay_imports.is_ok() && e.exports.is_ok());
                    }
                    1 => {
                        assert_eq!(
                            e.delay_imports,
                            Err(if rows_first {
                                PeDelayImportEvidenceError::OutputRowsExceeded { rows: 5, limit: 4 }
                            } else {
                                PeDelayImportEvidenceError::OutputTextExceeded {
                                    bytes: 23,
                                    limit: 22,
                                }
                            })
                        );
                        assert!(e.static_imports.is_ok() && e.exports.is_ok());
                    }
                    _ => {
                        assert_eq!(
                            e.exports,
                            Err(if rows_first {
                                PeExportEvidenceError::OutputRowsExceeded { rows: 8, limit: 7 }
                            } else {
                                PeExportEvidenceError::OutputTextExceeded {
                                    bytes: 35,
                                    limit: 34,
                                }
                            })
                        );
                        assert!(e.static_imports.is_ok() && e.delay_imports.is_ok());
                    }
                }
            }
        }
        for mask in 1..8 {
            let mut l = limits();
            if mask & 1 != 0 {
                l.static_imports = output(0, 0);
            }
            if mask & 2 != 0 {
                l.delay_imports = output(5, 0);
            }
            if mask & 4 != 0 {
                l.exports = output(0, 0);
            }
            let e = inspect_pe_module_evidence(&b, l).unwrap();
            assert_eq!(e, individual(&b, l));
            assert_eq!(e.static_imports.is_err(), mask & 1 != 0);
            assert_eq!(e.delay_imports.is_err(), mask & 2 != 0);
            assert_eq!(e.exports.is_err(), mask & 4 != 0);
        }
    }
}

#[test]
fn zero_output_limits_keep_absence_and_present_empty_distinct() {
    for plus in [false, true] {
        let l = PeModuleEvidenceLimits {
            max_input_bytes: 8192,
            static_imports: output(0, 0),
            delay_imports: output(0, 0),
            exports: output(0, 0),
        };
        let mut absent = fixture(plus);
        for i in [0, 1, 13] {
            absent[slot(plus) + i * 8..slot(plus) + i * 8 + 8].fill(0);
        }
        let a = inspect_pe_module_evidence(&absent, l).unwrap();
        assert_eq!(a, individual(&absent, l));
        assert_eq!(a.delay_imports.as_ref().unwrap().descriptors, Ok(None));
        assert_eq!(a.exports.as_ref().unwrap().addresses, Ok(None));
        let mut empty = fixture(plus);
        for (rva, n) in [(0x1000, 40), (0x2000, 40), (0x2800, 64)] {
            empty[offset(rva)..offset(rva) + n].fill(0);
        }
        let e = inspect_pe_module_evidence(&empty, l).unwrap();
        assert_eq!(e, individual(&empty, l));
        assert_eq!(a.static_imports, e.static_imports);
        assert!(e.delay_imports.unwrap().descriptors.unwrap().is_some());
        assert!(e.exports.unwrap().addresses.unwrap().is_some());
    }
}

#[test]
fn admitted_malformed_and_empty_inputs_keep_digest_and_all_reader_errors() {
    for bytes in [
        Vec::new(),
        vec![0; 64],
        fixture(false)[..180].to_vec(),
        fixture(true)[..180].to_vec(),
    ] {
        let l = PeModuleEvidenceLimits {
            max_input_bytes: u64::try_from(bytes.len()).unwrap(),
            static_imports: output(0, 0),
            delay_imports: output(0, 0),
            exports: output(0, 0),
        };
        let e = inspect_pe_module_evidence(&bytes, l).unwrap();
        assert_eq!(e, individual(&bytes, l));
        assert_eq!(e.fingerprinted.byte_length, l.max_input_bytes);
        assert!(e.static_imports.unwrap().lookups.is_err());
        assert!(e.delay_imports.unwrap().lookups.is_err());
        assert!(e.exports.unwrap().names.is_err());
    }
}
