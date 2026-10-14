use super::stopped_support;
use ring3_engine::{
    cpu::x86::decode::{DecodeError, decode_one},
    memory::GuestAddress,
    process::EngineInstance,
};

fn ram(engine: &EngineInstance) -> [Vec<u8>; 2] {
    [0x1000, 0x3000].map(|page| {
        let mut bytes = vec![0; 4096];
        engine
            .memory()
            .unwrap()
            .read(GuestAddress(page), &mut bytes)
            .unwrap();
        bytes
    })
}

fn prepares(bytes: &[u8]) {
    for resident in [false, true] {
        let mut engine = stopped_support::stopped(bytes, 0x3000);
        assert!(matches!(
            decode_one(engine.memory().unwrap(), GuestAddress(0x3000)),
            Err(DecodeError::Unsupported(_))
        ));
        let before = engine.arena().to_vec();
        let before_ram = ram(&engine);
        if resident {
            let id = engine
                .compile_stopped_resident_entry(stopped_support::KEY)
                .expect("qualified instruction must prepare resident fault material");
            assert_ne!(id.get(), 0);
            assert_eq!(
                &engine.resident_bytes(id.get()).unwrap()[..8],
                b"\0asm\x01\0\0\0"
            );
            assert_eq!(engine.generation(), 1);
        } else {
            assert_eq!(
                engine.compile_stopped_entry(stopped_support::KEY, 1),
                Ok(2),
                "{bytes:02x?}"
            );
            assert_eq!(&engine.artifact_bytes().unwrap()[..8], b"\0asm\x01\0\0\0");
        }
        assert_eq!(
            engine.arena(),
            before,
            "preparation changes no CPU or helper state"
        );
        assert_eq!(ram(&engine), before_ram, "preparation changes no guest RAM");
    }
}

#[test]
fn absent_lwp_prepares_ud_before_profiling_or_operand_effects() {
    for bytes in [
        &[0x8f, 0xe9, 0x78, 0x12, 0xc0][..],
        &[0x8f, 0xe9, 0x78, 0x12, 0xc8],
        &[0x8f, 0xea, 0x78, 0x12, 0xc1, 0, 0, 0, 0],
        &[0x64, 0x8f, 0xea, 0x78, 0x12, 0x08, 0, 0, 0, 0],
    ] {
        prepares(bytes);
    }
}
#[test]
fn absent_pconfig_prepares_ud_before_implicit_key_memory() {
    prepares(&[0x0f, 0x01, 0xc5]);
}
#[test]
fn absent_broadcast_tlb_forms_prepare_ud_before_privilege_or_register_checks() {
    for bytes in [
        &[0x0f, 0x01, 0xfe][..],
        &[0x67, 0x0f, 0x01, 0xfe],
        &[0x0f, 0x01, 0xff],
    ] {
        prepares(bytes);
    }
}
#[test]
fn unsupported_wbnoinvd_prepares_wbinvd_privilege_gp() {
    prepares(&[0xf3, 0x0f, 0x09]);
}
#[test]
fn existing_wbinvd_and_invd_privilege_controls_prepare() {
    for bytes in [&[0x0f, 0x09][..], &[0x0f, 0x08]] {
        prepares(bytes);
    }
}
#[test]
fn duplicate_prefix_refusal_and_separately_qualified_tbm_preparation_stay_distinct() {
    for bytes in [
        &[0xf3, 0xf3, 0x0f, 0x09][..],
        &[0x8f, 0xea, 0x78, 0x10, 0xc1, 0, 0, 0, 0],
    ] {
        if bytes == [0x8f, 0xea, 0x78, 0x10, 0xc1, 0, 0, 0, 0] {
            prepares(bytes);
            continue;
        }
        for resident in [false, true] {
            let mut engine = stopped_support::stopped(bytes, 0x3000);
            let before = engine.arena().to_vec();
            let before_ram = ram(&engine);
            if resident {
                assert!(
                    engine
                        .compile_stopped_resident_entry(stopped_support::KEY)
                        .is_err()
                );
            } else {
                assert!(
                    engine
                        .compile_stopped_entry(stopped_support::KEY, 1)
                        .is_err()
                );
            }
            assert_eq!(engine.arena(), before);
            assert_eq!(ram(&engine), before_ram);
        }
    }
}

#[test]
fn absent_enqueue_and_history_forms_prepare_ud_without_platform_authority() {
    use iced_x86::{Code, CpuidFeature, Decoder, DecoderOptions};
    let rows = [
        (
            Code::Enqcmd_r32_m512,
            CpuidFeature::ENQCMD,
            &[0xf2, 0x0f, 0x38, 0xf8, 0x08][..],
        ),
        (
            Code::Enqcmd_r16_m512,
            CpuidFeature::ENQCMD,
            &[0x67, 0xf2, 0x0f, 0x38, 0xf8, 0x08][..],
        ),
        (
            Code::Enqcmds_r32_m512,
            CpuidFeature::ENQCMD,
            &[0xf3, 0x0f, 0x38, 0xf8, 0x08][..],
        ),
        (
            Code::Enqcmds_r16_m512,
            CpuidFeature::ENQCMD,
            &[0x67, 0xf3, 0x0f, 0x38, 0xf8, 0x08][..],
        ),
        (
            Code::Hreset_imm8,
            CpuidFeature::HRESET,
            &[0xf3, 0x0f, 0x3a, 0xf0, 0xc0, 0][..],
        ),
    ];
    // retain the exact five bytesets from the original public refusal witness.
    for (code, feature, bytes) in rows {
        let raw = Decoder::new(32, bytes, DecoderOptions::NONE).decode();
        assert_eq!(raw.code(), code);
        assert_eq!(raw.len(), bytes.len());
        assert_eq!(raw.cpuid_features(), &[feature]);
        assert!(!raw.has_rep_prefix() && !raw.has_repne_prefix());
    }
    for (_, _, bytes) in rows {
        prepares(bytes);
    }
}

#[test]
fn absent_memory_commit_and_nonserializing_msr_prepare_without_platform_defaults() {
    use iced_x86::{Code, CpuidFeature, Decoder, DecoderOptions};
    for (code, feature, bytes) in [
        (
            Code::Mcommit,
            CpuidFeature::MCOMMIT,
            &[0xf3, 0x0f, 0x01, 0xfa][..],
        ),
        (
            Code::Wrmsrns,
            CpuidFeature::WRMSRNS,
            &[0x0f, 0x01, 0xc6][..],
        ),
    ] {
        for bitness in [16, 32] {
            let raw = Decoder::new(bitness, bytes, DecoderOptions::NONE).decode();
            assert_eq!(raw.code(), code);
            assert_eq!(raw.cpuid_features(), &[feature]);
            assert_eq!(raw.len(), bytes.len());
            assert_eq!(raw.op_count(), 0);
            assert!(!raw.has_rep_prefix() && !raw.has_repne_prefix());
        }
        prepares(bytes);
    }
}
