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
        bound_imports: output(0, 0),
    }
}
fn individual(bytes: &[u8], l: PeModuleEvidenceLimits) -> PeModuleEvidence {
    PeModuleEvidence {
        bound_imports: ring3_core::inspect_pe_bound_imports(
            bytes,
            ring3_core::PeBoundImportEvidenceLimits {
                max_input_bytes: l.max_input_bytes,
                max_output_rows: l.bound_imports.max_rows,
                max_output_text_bytes: l.bound_imports.max_text_bytes,
            },
        ),
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
        assert_eq!(before.bound_imports, after.bound_imports);
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
            bound_imports: output(0, 0),
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
            bound_imports: output(0, 0),
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
            bound_imports: output(0, 0),
        };
        let e = inspect_pe_module_evidence(&bytes, l).unwrap();
        assert_eq!(e, individual(&bytes, l));
        assert_eq!(e.fingerprinted.byte_length, l.max_input_bytes);
        assert!(e.static_imports.unwrap().lookups.is_err());
        assert!(e.delay_imports.unwrap().lookups.is_err());
        assert!(e.exports.unwrap().names.is_err());
    }
}

struct CompiledModule {
    variable: &'static str,
    length: u64,
    digest: &'static str,
    kind: ring3_core::PeKind,
    declarations: (u16, u16, u32, u16, u16),
    caps: [u64; 6],
}

fn compiled_modules() -> Vec<(CompiledModule, Vec<u8>)> {
    let fixtures = [
        CompiledModule {
            variable: "RING3_IMPORT_PE32_FIXTURE",
            length: 2048,
            digest: "38fc89db15266cf8361f7c0096fd3ecaee142ef28645f6c8540156bc8515a69e",
            kind: ring3_core::PeKind::Pe32,
            declarations: (332, 259, 4096, 3, 33024),
            caps: [3, 39, 0, 0, 0, 0],
        },
        CompiledModule {
            variable: "RING3_IMPORT_PE32PLUS_FIXTURE",
            length: 2048,
            digest: "a8214ec448366c86c1dc289f624316409a6ca34f695639365e796025187db8a1",
            kind: ring3_core::PeKind::Pe32Plus,
            declarations: (34404, 35, 4096, 3, 33056),
            caps: [3, 39, 0, 0, 0, 0],
        },
        CompiledModule {
            variable: "RING3_DELAY_PE32_FIXTURE",
            length: 2560,
            digest: "ab2c15214a85f8593608a30ca9a0b462ec61a04e10b4abd1e86e4869210640b8",
            kind: ring3_core::PeKind::Pe32,
            declarations: (332, 259, 4112, 3, 33024),
            caps: [0, 0, 4, 33, 0, 0],
        },
        CompiledModule {
            variable: "RING3_DELAY_PE32PLUS_FIXTURE",
            length: 3072,
            digest: "5845501b0acf7d97c5470ce06bc738a2d2e3ce3aac64f69c164cbda8cb942e17",
            kind: ring3_core::PeKind::Pe32Plus,
            declarations: (34404, 35, 4112, 3, 33056),
            caps: [0, 0, 4, 33, 0, 0],
        },
        CompiledModule {
            variable: "RING3_EXPORT_FORWARD_PE32_FIXTURE",
            length: 2048,
            digest: "712d2aa28d94475c0008c21d31f1d38964b8531f427911da85df34957c13be8d",
            kind: ring3_core::PeKind::Pe32,
            declarations: (332, 8451, 0, 2, 256),
            caps: [0, 0, 0, 0, 8, 101],
        },
        CompiledModule {
            variable: "RING3_EXPORT_FORWARD_PE32PLUS_FIXTURE",
            length: 2048,
            digest: "029ebe773e592200afe56c40fca5fbe28a641e260348d8870b8f949f0e52420c",
            kind: ring3_core::PeKind::Pe32Plus,
            declarations: (34404, 8227, 0, 2, 288),
            caps: [0, 0, 0, 0, 8, 101],
        },
    ];
    let paths: Vec<_> = fixtures
        .iter()
        .map(|f| {
            std::env::var_os(f.variable)
                .expect("all six explicit compiled module paths are required")
        })
        .collect();
    fixtures
        .into_iter()
        .zip(paths)
        .map(|(f, path)| {
            let bytes = std::fs::read(path).expect("compiled module fixture must be readable");
            (f, bytes)
        })
        .collect()
}

fn compiled_limits(f: &CompiledModule) -> PeModuleEvidenceLimits {
    PeModuleEvidenceLimits {
        max_input_bytes: f.length,
        static_imports: output(f.caps[0], f.caps[1]),
        delay_imports: output(f.caps[2], f.caps[3]),
        exports: output(f.caps[4], f.caps[5]),
        bound_imports: output(0, 0),
    }
}

#[test]
#[ignore = "requires explicit self-authored compiled fixtures"]
fn generated_owned_module_evidence_preserves_compiled_observations() {
    use std::fmt::Write as _;
    for (fixture, mut bytes) in compiled_modules() {
        assert_eq!(
            u64::try_from(bytes.len()).unwrap(),
            fixture.length,
            "{}",
            fixture.variable
        );
        let limits = compiled_limits(&fixture);
        let expected = individual(&bytes, limits);
        let original = bytes.clone();
        let evidence = inspect_pe_module_evidence(&bytes, limits).unwrap();
        assert_eq!(evidence, expected, "{}", fixture.variable);
        assert_eq!(
            evidence,
            inspect_pe_module_evidence(&bytes, limits).unwrap()
        );
        assert_eq!(bytes, original);
        bytes.fill(0xee);
        drop(bytes);
        assert_eq!(evidence.clone(), expected);
        assert_eq!(evidence.fingerprinted.byte_length, fixture.length);
        let mut digest = String::with_capacity(64);
        for byte in evidence.fingerprinted.digest {
            write!(digest, "{byte:02x}").unwrap();
        }
        assert_eq!(digest, fixture.digest);
        let prefix = evidence.fingerprinted.evidence.prefix.unwrap();
        let optional = evidence.fingerprinted.evidence.optional.unwrap();
        assert_eq!(prefix.kind.value, fixture.kind);
        assert_eq!(
            (
                prefix.machine.value,
                prefix.characteristics.value,
                optional.entry_rva.value.get(),
                optional.subsystem.value,
                optional.dll_characteristics.value
            ),
            fixture.declarations
        );
        assert_eq!(optional.directory_count.value, 16);
        assert_eq!(evidence.fingerprinted.evidence.clr, Ok(None));
        assert_eq!(
            evidence.bound_imports,
            Ok(ring3_core::PeBoundImportEvidence {
                total_rows: 0,
                total_text_bytes: 0,
                descriptors: Ok(None),
                names: Ok(None),
            })
        );
        let static_imports = evidence.static_imports.unwrap();
        let delay_imports = evidence.delay_imports.unwrap();
        let exports = evidence.exports.unwrap();
        assert_eq!(
            [
                static_imports.total_rows,
                static_imports.total_text_bytes,
                delay_imports.total_rows,
                delay_imports.total_text_bytes,
                exports.total_rows,
                exports.total_text_bytes
            ],
            fixture.caps
        );
        assert!(static_imports.descriptors.is_ok() && static_imports.lookups.is_ok());
        assert!(
            delay_imports.descriptors.is_ok()
                && delay_imports.names.is_ok()
                && delay_imports.lookups.is_ok()
        );
        assert!(exports.directory.is_ok() && exports.addresses.is_ok() && exports.names.is_ok());
    }
}

#[test]
#[ignore = "requires explicit self-authored compiled fixtures"]
fn generated_owned_module_refusals_preserve_family_isolation() {
    for (f, bytes) in compiled_modules() {
        let exact = compiled_limits(&f);
        let expected = individual(&bytes, exact);
        assert_eq!(inspect_pe_module_evidence(&bytes, exact).unwrap(), expected);
        let too_small = PeModuleEvidenceLimits {
            max_input_bytes: f.length - 1,
            static_imports: output(0, 0),
            delay_imports: output(0, 0),
            exports: output(0, 0),
            bound_imports: output(0, 0),
        };
        assert_eq!(
            inspect_pe_module_evidence(&bytes, too_small),
            Err(PeFingerprintError::InputTooLarge {
                length: f.length,
                limit: f.length - 1
            })
        );
        for family in 0..3 {
            let (rows, text_bytes) = (f.caps[family * 2], f.caps[family * 2 + 1]);
            if rows == 0 {
                assert_eq!(text_bytes, 0);
                continue;
            }
            assert!(text_bytes > 0);
            for row_refusal in [true, false] {
                let mut l = exact;
                let cap = match family {
                    0 => &mut l.static_imports,
                    1 => &mut l.delay_imports,
                    _ => &mut l.exports,
                };
                *cap = if row_refusal {
                    output(rows - 1, 0)
                } else {
                    output(rows, text_bytes - 1)
                };
                let e = inspect_pe_module_evidence(&bytes, l).unwrap();
                assert_eq!(e, individual(&bytes, l));
                assert_eq!(e.fingerprinted, expected.fingerprinted);
                match family {
                    0 => {
                        assert_eq!(
                            e.static_imports,
                            Err(if row_refusal {
                                PeStaticImportEvidenceError::OutputRowsExceeded {
                                    rows,
                                    limit: rows - 1,
                                }
                            } else {
                                PeStaticImportEvidenceError::OutputTextExceeded {
                                    bytes: text_bytes,
                                    limit: text_bytes - 1,
                                }
                            })
                        );
                        assert_eq!(e.delay_imports, expected.delay_imports);
                        assert_eq!(e.exports, expected.exports);
                    }
                    1 => {
                        assert_eq!(
                            e.delay_imports,
                            Err(if row_refusal {
                                PeDelayImportEvidenceError::OutputRowsExceeded {
                                    rows,
                                    limit: rows - 1,
                                }
                            } else {
                                PeDelayImportEvidenceError::OutputTextExceeded {
                                    bytes: text_bytes,
                                    limit: text_bytes - 1,
                                }
                            })
                        );
                        assert_eq!(e.static_imports, expected.static_imports);
                        assert_eq!(e.exports, expected.exports);
                    }
                    _ => {
                        assert_eq!(
                            e.exports,
                            Err(if row_refusal {
                                PeExportEvidenceError::OutputRowsExceeded {
                                    rows,
                                    limit: rows - 1,
                                }
                            } else {
                                PeExportEvidenceError::OutputTextExceeded {
                                    bytes: text_bytes,
                                    limit: text_bytes - 1,
                                }
                            })
                        );
                        assert_eq!(e.static_imports, expected.static_imports);
                        assert_eq!(e.delay_imports, expected.delay_imports);
                    }
                }
            }
        }
    }
}

fn with_bound(plus: bool) -> Vec<u8> {
    let mut b = fixture(plus);
    put(&mut b, slot(plus) + 88, 0x2c00);
    put(&mut b, slot(plus) + 92, 32);
    for (index, timestamp, name, count) in
        [(0, 123, 128, 1), (1, 456, 144, 0xbeef), (2, 789, 160, 0)]
    {
        let o = offset(0x2c00) + index * 8;
        put(&mut b, o, timestamp);
        word(&mut b, o + 4, name);
        word(&mut b, o + 6, count);
    }
    text(&mut b, 0x2c80, b"Bound.DLL\0");
    text(&mut b, 0x2c90, b"Other.DLL\0");
    text(&mut b, 0x2ca0, b"Tail.DLL\0");
    b
}

#[test]
fn bound_family_owns_same_input_metadata_after_release() {
    for plus in [false, true] {
        let mut bytes = with_bound(plus);
        let before = bytes.clone();
        let mut caps = limits();
        caps.bound_imports = output(9, 26);
        let expected = individual(&bytes, caps);
        let actual = inspect_pe_module_evidence(&bytes, caps).unwrap();
        assert_eq!(actual, inspect_pe_module_evidence(&bytes, caps).unwrap());
        assert_eq!(bytes, before);
        bytes.fill(0);
        drop(bytes);
        assert_eq!(actual, expected);
        let bound = actual.bound_imports.unwrap();
        assert_eq!((bound.total_rows, bound.total_text_bytes), (9, 26));
        let raw = bound.descriptors.unwrap().unwrap();
        assert_eq!(raw.descriptors.len(), 2);
        assert_eq!(raw.descriptors[0].forwarder_refs[0].reserved, 0xbeef);
        let names = bound.names.unwrap().unwrap();
        assert_eq!(names.table, raw);
        assert_eq!(
            names
                .names
                .iter()
                .map(|n| n.dll_name.as_str())
                .collect::<Vec<_>>(),
            ["Bound.DLL", "Other.DLL", "Tail.DLL"]
        );
        assert_eq!(names.names[2].name_rva.get(), 0x2ca0);
        assert_eq!(names.names[2].name_file_offset.get(), 7840);
    }
}

#[test]
fn all_four_family_refusal_subsets_keep_other_complete_results() {
    use ring3_core::PeBoundImportEvidenceError as BoundError;
    for plus in [false, true] {
        let bytes = with_bound(plus);
        for text_refusal in [false, true] {
            for mask in 0..16 {
                let mut caps = limits();
                caps.bound_imports = output(9, 26);
                for (bit, cap, rows, text) in [
                    (1, &mut caps.static_imports, 5, 32),
                    (2, &mut caps.delay_imports, 5, 23),
                    (4, &mut caps.exports, 8, 35),
                    (8, &mut caps.bound_imports, 9, 26),
                ] {
                    if mask & bit != 0 {
                        *cap = if text_refusal {
                            output(rows, text - 1)
                        } else {
                            output(rows - 1, 0)
                        };
                    }
                }
                let actual = inspect_pe_module_evidence(&bytes, caps).unwrap();
                assert_eq!(actual, individual(&bytes, caps));
                assert_eq!(actual.static_imports.is_err(), mask & 1 != 0);
                assert_eq!(actual.delay_imports.is_err(), mask & 2 != 0);
                assert_eq!(actual.exports.is_err(), mask & 4 != 0);
                assert_eq!(actual.bound_imports.is_err(), mask & 8 != 0);
                if mask & 8 != 0 {
                    assert_eq!(
                        actual.bound_imports,
                        Err(if text_refusal {
                            BoundError::OutputTextExceeded {
                                bytes: 26,
                                limit: 25,
                            }
                        } else {
                            BoundError::OutputRowsExceeded { rows: 9, limit: 8 }
                        })
                    );
                }
            }
        }
        let mut caps = limits();
        caps.max_input_bytes = 8191;
        assert_eq!(
            inspect_pe_module_evidence(&bytes, caps),
            Err(PeFingerprintError::InputTooLarge {
                length: 8192,
                limit: 8191
            })
        );
    }
}

#[test]
fn bound_inner_errors_and_empty_tables_remain_independent() {
    for plus in [false, true] {
        let mut caps = limits();
        caps.bound_imports = output(9, 26);
        for mode in 0..4 {
            let mut bytes = with_bound(plus);
            match mode {
                0 => bytes[offset(0x2ca0)] = 0xff,
                1 => put(&mut bytes, slot(plus) + 92, 24),
                2 => bytes[slot(plus) + 88..slot(plus) + 96].fill(0),
                _ => bytes[offset(0x2c00)..offset(0x2c00) + 32].fill(0),
            }
            let actual = inspect_pe_module_evidence(&bytes, caps).unwrap();
            assert_eq!(actual, individual(&bytes, caps));
            assert!(
                actual.static_imports.is_ok()
                    && actual.delay_imports.is_ok()
                    && actual.exports.is_ok()
            );
            let bound = actual.bound_imports.unwrap();
            assert_eq!(
                (bound.total_rows, bound.total_text_bytes),
                (if mode == 0 { 3 } else { 0 }, 0)
            );
            match mode {
                0 => {
                    assert_eq!(bound.descriptors.unwrap().unwrap().descriptors.len(), 2);
                    assert_eq!(
                        bound.names,
                        Err(ring3_core::PeBoundImportNameError::NonAsciiDllName {
                            location: ring3_core::PeBoundImportNameLocation::Descriptor {
                                descriptor_index: 1
                            },
                            name_rva: ring3_core::RelativeVirtualAddress::new(0x2ca0),
                            offset: 0,
                            byte: 0xff,
                        })
                    );
                }
                1 => {
                    assert!(bound.descriptors.is_err() && bound.names.is_err());
                }
                2 => {
                    assert_eq!(bound.descriptors, Ok(None));
                    assert_eq!(bound.names, Ok(None));
                }
                _ => {
                    assert!(bound.descriptors.unwrap().unwrap().descriptors.is_empty());
                    assert!(bound.names.unwrap().unwrap().names.is_empty());
                }
            }
        }
    }
}
