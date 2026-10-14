use crate::{
    cpu::x86::{
        ControlOperation, SegmentCache, SegmentContext,
        capabilities::CPU_ID_LEAVES,
        decode::{
            BoundDecode, CodeFetchContext, DecodeError, DecodePolicy, decode_bound,
            decode_for_execution, decode_one,
        },
        environment::USER_MODE_ENVIRONMENT,
        ir::Operation,
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};
use iced_x86::{Code, CpuidFeature, Decoder, DecoderOptions};
fn rows() -> [(Code, CpuidFeature, &'static [u8]); 64] {
    [
        (
            Code::VEX_Vpdpbusd_xmm_xmm_xmmm128,
            CpuidFeature::AVX_VNNI,
            &[0xc4, 0xe2, 0x79, 0x50, 0xc1],
        ),
        (
            Code::VEX_Vpdpbusd_ymm_ymm_ymmm256,
            CpuidFeature::AVX_VNNI,
            &[0xc4, 0xe2, 0x7d, 0x50, 0xc1],
        ),
        (
            Code::VEX_Vpdpbusds_xmm_xmm_xmmm128,
            CpuidFeature::AVX_VNNI,
            &[0xc4, 0xe2, 0x79, 0x51, 0xc1],
        ),
        (
            Code::VEX_Vpdpbusds_ymm_ymm_ymmm256,
            CpuidFeature::AVX_VNNI,
            &[0xc4, 0xe2, 0x7d, 0x51, 0xc1],
        ),
        (
            Code::VEX_Vpdpwssd_xmm_xmm_xmmm128,
            CpuidFeature::AVX_VNNI,
            &[0xc4, 0xe2, 0x79, 0x52, 0xc1],
        ),
        (
            Code::VEX_Vpdpwssd_ymm_ymm_ymmm256,
            CpuidFeature::AVX_VNNI,
            &[0xc4, 0xe2, 0x7d, 0x52, 0xc1],
        ),
        (
            Code::VEX_Vpdpwssds_xmm_xmm_xmmm128,
            CpuidFeature::AVX_VNNI,
            &[0xc4, 0xe2, 0x79, 0x53, 0xc1],
        ),
        (
            Code::VEX_Vpdpwssds_ymm_ymm_ymmm256,
            CpuidFeature::AVX_VNNI,
            &[0xc4, 0xe2, 0x7d, 0x53, 0xc1],
        ),
        (
            Code::Aadd_m32_r32,
            CpuidFeature::RAO_INT,
            &[0x0f, 0x38, 0xfc, 0x08],
        ),
        (
            Code::Aand_m32_r32,
            CpuidFeature::RAO_INT,
            &[0x66, 0x0f, 0x38, 0xfc, 0x08],
        ),
        (
            Code::Axor_m32_r32,
            CpuidFeature::RAO_INT,
            &[0xf3, 0x0f, 0x38, 0xfc, 0x08],
        ),
        (
            Code::Aor_m32_r32,
            CpuidFeature::RAO_INT,
            &[0xf2, 0x0f, 0x38, 0xfc, 0x08],
        ),
        (
            Code::VEX_Vpdpbuud_xmm_xmm_xmmm128,
            CpuidFeature::AVX_VNNI_INT8,
            &[0xc4, 0xe2, 0x78, 0x50, 0xc1],
        ),
        (
            Code::VEX_Vpdpbuud_ymm_ymm_ymmm256,
            CpuidFeature::AVX_VNNI_INT8,
            &[0xc4, 0xe2, 0x7c, 0x50, 0xc1],
        ),
        (
            Code::VEX_Vpdpbsud_xmm_xmm_xmmm128,
            CpuidFeature::AVX_VNNI_INT8,
            &[0xc4, 0xe2, 0x7a, 0x50, 0xc1],
        ),
        (
            Code::VEX_Vpdpbsud_ymm_ymm_ymmm256,
            CpuidFeature::AVX_VNNI_INT8,
            &[0xc4, 0xe2, 0x7e, 0x50, 0xc1],
        ),
        (
            Code::VEX_Vpdpbssd_xmm_xmm_xmmm128,
            CpuidFeature::AVX_VNNI_INT8,
            &[0xc4, 0xe2, 0x7b, 0x50, 0xc1],
        ),
        (
            Code::VEX_Vpdpbssd_ymm_ymm_ymmm256,
            CpuidFeature::AVX_VNNI_INT8,
            &[0xc4, 0xe2, 0x7f, 0x50, 0xc1],
        ),
        (
            Code::VEX_Vpdpbuuds_xmm_xmm_xmmm128,
            CpuidFeature::AVX_VNNI_INT8,
            &[0xc4, 0xe2, 0x78, 0x51, 0xc1],
        ),
        (
            Code::VEX_Vpdpbuuds_ymm_ymm_ymmm256,
            CpuidFeature::AVX_VNNI_INT8,
            &[0xc4, 0xe2, 0x7c, 0x51, 0xc1],
        ),
        (
            Code::VEX_Vpdpbsuds_xmm_xmm_xmmm128,
            CpuidFeature::AVX_VNNI_INT8,
            &[0xc4, 0xe2, 0x7a, 0x51, 0xc1],
        ),
        (
            Code::VEX_Vpdpbsuds_ymm_ymm_ymmm256,
            CpuidFeature::AVX_VNNI_INT8,
            &[0xc4, 0xe2, 0x7e, 0x51, 0xc1],
        ),
        (
            Code::VEX_Vpdpbssds_xmm_xmm_xmmm128,
            CpuidFeature::AVX_VNNI_INT8,
            &[0xc4, 0xe2, 0x7b, 0x51, 0xc1],
        ),
        (
            Code::VEX_Vpdpbssds_ymm_ymm_ymmm256,
            CpuidFeature::AVX_VNNI_INT8,
            &[0xc4, 0xe2, 0x7f, 0x51, 0xc1],
        ),
        (
            Code::VEX_Vcvtneps2bf16_xmm_xmmm128,
            CpuidFeature::AVX_NE_CONVERT,
            &[0xc4, 0xe2, 0x7a, 0x72, 0xc1],
        ),
        (
            Code::VEX_Vcvtneps2bf16_xmm_ymmm256,
            CpuidFeature::AVX_NE_CONVERT,
            &[0xc4, 0xe2, 0x7e, 0x72, 0xc1],
        ),
        (
            Code::VEX_Vcvtneoph2ps_xmm_m128,
            CpuidFeature::AVX_NE_CONVERT,
            &[0xc4, 0xe2, 0x78, 0xb0, 0x00],
        ),
        (
            Code::VEX_Vcvtneoph2ps_ymm_m256,
            CpuidFeature::AVX_NE_CONVERT,
            &[0xc4, 0xe2, 0x7c, 0xb0, 0x00],
        ),
        (
            Code::VEX_Vcvtneeph2ps_xmm_m128,
            CpuidFeature::AVX_NE_CONVERT,
            &[0xc4, 0xe2, 0x79, 0xb0, 0x00],
        ),
        (
            Code::VEX_Vcvtneeph2ps_ymm_m256,
            CpuidFeature::AVX_NE_CONVERT,
            &[0xc4, 0xe2, 0x7d, 0xb0, 0x00],
        ),
        (
            Code::VEX_Vcvtneebf162ps_xmm_m128,
            CpuidFeature::AVX_NE_CONVERT,
            &[0xc4, 0xe2, 0x7a, 0xb0, 0x00],
        ),
        (
            Code::VEX_Vcvtneebf162ps_ymm_m256,
            CpuidFeature::AVX_NE_CONVERT,
            &[0xc4, 0xe2, 0x7e, 0xb0, 0x00],
        ),
        (
            Code::VEX_Vcvtneobf162ps_xmm_m128,
            CpuidFeature::AVX_NE_CONVERT,
            &[0xc4, 0xe2, 0x7b, 0xb0, 0x00],
        ),
        (
            Code::VEX_Vcvtneobf162ps_ymm_m256,
            CpuidFeature::AVX_NE_CONVERT,
            &[0xc4, 0xe2, 0x7f, 0xb0, 0x00],
        ),
        (
            Code::VEX_Vbcstnesh2ps_xmm_m16,
            CpuidFeature::AVX_NE_CONVERT,
            &[0xc4, 0xe2, 0x79, 0xb1, 0x00],
        ),
        (
            Code::VEX_Vbcstnesh2ps_ymm_m16,
            CpuidFeature::AVX_NE_CONVERT,
            &[0xc4, 0xe2, 0x7d, 0xb1, 0x00],
        ),
        (
            Code::VEX_Vbcstnebf162ps_xmm_m16,
            CpuidFeature::AVX_NE_CONVERT,
            &[0xc4, 0xe2, 0x7a, 0xb1, 0x00],
        ),
        (
            Code::VEX_Vbcstnebf162ps_ymm_m16,
            CpuidFeature::AVX_NE_CONVERT,
            &[0xc4, 0xe2, 0x7e, 0xb1, 0x00],
        ),
        (
            Code::VEX_Vpmadd52luq_xmm_xmm_xmmm128,
            CpuidFeature::AVX_IFMA,
            &[0xc4, 0xe2, 0xf9, 0xb4, 0xc1],
        ),
        (
            Code::VEX_Vpmadd52luq_ymm_ymm_ymmm256,
            CpuidFeature::AVX_IFMA,
            &[0xc4, 0xe2, 0xfd, 0xb4, 0xc1],
        ),
        (
            Code::VEX_Vpmadd52huq_xmm_xmm_xmmm128,
            CpuidFeature::AVX_IFMA,
            &[0xc4, 0xe2, 0xf9, 0xb5, 0xc1],
        ),
        (
            Code::VEX_Vpmadd52huq_ymm_ymm_ymmm256,
            CpuidFeature::AVX_IFMA,
            &[0xc4, 0xe2, 0xfd, 0xb5, 0xc1],
        ),
        (
            Code::VEX_Vsha512rnds2_ymm_ymm_xmm,
            CpuidFeature::SHA512,
            &[0xc4, 0xe2, 0x7f, 0xcb, 0xc1],
        ),
        (
            Code::VEX_Vsha512msg1_ymm_xmm,
            CpuidFeature::SHA512,
            &[0xc4, 0xe2, 0x7f, 0xcc, 0xc1],
        ),
        (
            Code::VEX_Vsha512msg2_ymm_ymm,
            CpuidFeature::SHA512,
            &[0xc4, 0xe2, 0x7f, 0xcd, 0xc1],
        ),
        (
            Code::VEX_Vpdpwuud_xmm_xmm_xmmm128,
            CpuidFeature::AVX_VNNI_INT16,
            &[0xc4, 0xe2, 0x78, 0xd2, 0xc1],
        ),
        (
            Code::VEX_Vpdpwuud_ymm_ymm_ymmm256,
            CpuidFeature::AVX_VNNI_INT16,
            &[0xc4, 0xe2, 0x7c, 0xd2, 0xc1],
        ),
        (
            Code::VEX_Vpdpwusd_xmm_xmm_xmmm128,
            CpuidFeature::AVX_VNNI_INT16,
            &[0xc4, 0xe2, 0x79, 0xd2, 0xc1],
        ),
        (
            Code::VEX_Vpdpwusd_ymm_ymm_ymmm256,
            CpuidFeature::AVX_VNNI_INT16,
            &[0xc4, 0xe2, 0x7d, 0xd2, 0xc1],
        ),
        (
            Code::VEX_Vpdpwsud_xmm_xmm_xmmm128,
            CpuidFeature::AVX_VNNI_INT16,
            &[0xc4, 0xe2, 0x7a, 0xd2, 0xc1],
        ),
        (
            Code::VEX_Vpdpwsud_ymm_ymm_ymmm256,
            CpuidFeature::AVX_VNNI_INT16,
            &[0xc4, 0xe2, 0x7e, 0xd2, 0xc1],
        ),
        (
            Code::VEX_Vpdpwuuds_xmm_xmm_xmmm128,
            CpuidFeature::AVX_VNNI_INT16,
            &[0xc4, 0xe2, 0x78, 0xd3, 0xc1],
        ),
        (
            Code::VEX_Vpdpwuuds_ymm_ymm_ymmm256,
            CpuidFeature::AVX_VNNI_INT16,
            &[0xc4, 0xe2, 0x7c, 0xd3, 0xc1],
        ),
        (
            Code::VEX_Vpdpwusds_xmm_xmm_xmmm128,
            CpuidFeature::AVX_VNNI_INT16,
            &[0xc4, 0xe2, 0x79, 0xd3, 0xc1],
        ),
        (
            Code::VEX_Vpdpwusds_ymm_ymm_ymmm256,
            CpuidFeature::AVX_VNNI_INT16,
            &[0xc4, 0xe2, 0x7d, 0xd3, 0xc1],
        ),
        (
            Code::VEX_Vpdpwsuds_xmm_xmm_xmmm128,
            CpuidFeature::AVX_VNNI_INT16,
            &[0xc4, 0xe2, 0x7a, 0xd3, 0xc1],
        ),
        (
            Code::VEX_Vpdpwsuds_ymm_ymm_ymmm256,
            CpuidFeature::AVX_VNNI_INT16,
            &[0xc4, 0xe2, 0x7e, 0xd3, 0xc1],
        ),
        (
            Code::VEX_Vsm3msg1_xmm_xmm_xmmm128,
            CpuidFeature::SM3,
            &[0xc4, 0xe2, 0x78, 0xda, 0xc1],
        ),
        (
            Code::VEX_Vsm3msg2_xmm_xmm_xmmm128,
            CpuidFeature::SM3,
            &[0xc4, 0xe2, 0x79, 0xda, 0xc1],
        ),
        (
            Code::VEX_Vsm4key4_xmm_xmm_xmmm128,
            CpuidFeature::SM4,
            &[0xc4, 0xe2, 0x7a, 0xda, 0xc1],
        ),
        (
            Code::VEX_Vsm4key4_ymm_ymm_ymmm256,
            CpuidFeature::SM4,
            &[0xc4, 0xe2, 0x7e, 0xda, 0xc1],
        ),
        (
            Code::VEX_Vsm4rnds4_xmm_xmm_xmmm128,
            CpuidFeature::SM4,
            &[0xc4, 0xe2, 0x7b, 0xda, 0xc1],
        ),
        (
            Code::VEX_Vsm4rnds4_ymm_ymm_ymmm256,
            CpuidFeature::SM4,
            &[0xc4, 0xe2, 0x7f, 0xda, 0xc1],
        ),
        (
            Code::VEX_Vsm3rnds2_xmm_xmm_xmmm128_imm8,
            CpuidFeature::SM3,
            &[0xc4, 0xe3, 0x79, 0xde, 0xc1, 0x00],
        ),
    ]
}
fn executable(bytes: &[u8], pc: u32) -> AddressSpace {
    let mut memory = AddressSpace::new(2).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(pc & !0xfff), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory
}
#[test]
fn all_qualified_modern_codes_select_their_absent_bit_in_both_default_modes() {
    for default32 in [false, true] {
        for (code, feature, bytes) in rows() {
            for prefix in [&[][..], &[0x64, 0x67]] {
                let full = [prefix, bytes].concat();
                let raw =
                    Decoder::new(if default32 { 32 } else { 16 }, &full, DecoderOptions::NONE)
                        .decode();
                assert_eq!(raw.code(), code, "{full:02x?}");
                assert_eq!(raw.len(), full.len());
                assert!(raw.cpuid_features().contains(&feature));
                assert!(!raw.has_rep_prefix() && !raw.has_repne_prefix());
                let (function, subfunction, register, bit) = super::required(feature).unwrap();
                assert!(!CPU_ID_LEAVES.iter().any(|leaf| leaf.function == function
                    && leaf.subfunction == Some(subfunction)
                    && leaf.registers[register] & (1 << bit) != 0));
                let memory = executable(&full, 0x1000);
                let mut segments = SegmentContext::default();
                segments.state.selectors[4] = 0;
                segments.caches[4] = SegmentCache::from_bits(0).unwrap();
                if !default32 {
                    segments.caches[1] = SegmentCache::from_bits(0xf1bf_ffff).unwrap();
                }
                let cs = CodeFetchContext::from_segments(&segments).unwrap();
                assert!(matches!(
                    decode_bound(&memory, GuestAddress(0x1000), cs, DecodePolicy::Strict),
                    Err(DecodeError::Unsupported(_))
                ));
                let BoundDecode::Instruction(decoded) =
                    decode_bound(&memory, GuestAddress(0x1000), cs, DecodePolicy::Execution)
                        .unwrap()
                else {
                    panic!("complete code")
                };
                assert_eq!(
                    decoded.operation(),
                    &Operation::Control(ControlOperation::InvalidOpcode)
                );
                assert_eq!(decoded.segment_accesses(), 0);
                assert_eq!(usize::from(decoded.length()), full.len());
                assert_eq!(decoded.next_pc(), GuestAddress(0x1000 + full.len() as u32));
                assert!(memory.is_code_current(decoded.code_snapshot()));
            }
        }
    }
}
#[test]
fn modern_mapping_is_exact_and_keeps_supported_vector_neighbors() {
    for (feature, register, bit) in [
        (CpuidFeature::SHA512, 0, 0),
        (CpuidFeature::SM3, 0, 1),
        (CpuidFeature::SM4, 0, 2),
        (CpuidFeature::RAO_INT, 0, 3),
        (CpuidFeature::AVX_VNNI, 0, 4),
        (CpuidFeature::AVX_IFMA, 0, 23),
        (CpuidFeature::AVX_VNNI_INT8, 3, 4),
        (CpuidFeature::AVX_NE_CONVERT, 3, 5),
        (CpuidFeature::AVX_VNNI_INT16, 3, 10),
    ] {
        assert_eq!(super::required(feature), Some((7, 1, register, bit)));
    }
    for feature in [
        CpuidFeature::AVX,
        CpuidFeature::AVX2,
        CpuidFeature::FMA,
        CpuidFeature::AES,
        CpuidFeature::PCLMULQDQ,
        CpuidFeature::VAES,
        CpuidFeature::VPCLMULQDQ,
        CpuidFeature::GFNI,
    ] {
        assert_eq!(super::required(feature), None);
    }
    for bytes in [
        &[0xc5, 0xfd, 0xfe, 0xc1][..],      // AVX2 VPADDD
        &[0xc4, 0xe2, 0x79, 0x98, 0xc1],    // FMA3 VFMADD132PS
        &[0xc4, 0xe2, 0x79, 0xdc, 0xc1],    // VEX128 AES
        &[0xc4, 0xe3, 0x79, 0x44, 0xc1, 0], // VEX128 PCLMUL
    ] {
        let memory = executable(bytes, 0x1000);
        let strict = decode_one(&memory, GuestAddress(0x1000)).unwrap();
        assert!(matches!(strict.operation(), Operation::Avx(_)));
        assert_eq!(
            decode_for_execution(&memory, GuestAddress(0x1000))
                .unwrap()
                .operation(),
            strict.operation()
        );
    }
}
#[test]
fn final_immediate_or_operand_byte_and_prefix_validity_still_precede_availability() {
    for bytes in [
        &[0xc4, 0xe3, 0x79, 0xde, 0xc1, 0][..],
        &[0xf2, 0x0f, 0x38, 0xfc, 0x08],
        &[0xc4, 0xe2, 0x79, 0xb1, 0],
    ] {
        let pc = 0x2000 - bytes.len() as u32 + 1;
        let memory = executable(&bytes[..bytes.len() - 1], pc);
        let missing = decode_one(&memory, GuestAddress(pc)).unwrap_err();
        assert!(
            matches!(missing,DecodeError::Paging { attempted_length: length, error: crate::cpu::x86::CpuPagingError32::PageFault(_), .. } if usize::from(length) == bytes.len())
        );
        assert_eq!(
            decode_for_execution(&memory, GuestAddress(pc)).unwrap_err(),
            missing
        );
    }
    for (_, _, bytes) in rows() {
        let raw = Decoder::new(32, bytes, DecoderOptions::NONE).decode();
        let duplicate = [&[0x66, 0x66][..], bytes].concat();
        assert_eq!(
            crate::cpu::x86::decode::execution_faults::for_instruction(
                &raw,
                &duplicate,
                USER_MODE_ENVIRONMENT
            ),
            None
        );
        let locked = [&[0xf0][..], bytes].concat();
        let memory = executable(&locked, 0x1000);
        assert_eq!(
            decode_one(&memory, GuestAddress(0x1000)).unwrap_err(),
            DecodeError::InvalidEncoding
        );
        assert_eq!(
            decode_for_execution(&memory, GuestAddress(0x1000))
                .unwrap()
                .operation(),
            &Operation::Control(ControlOperation::InvalidOpcode)
        );
    }
    let memory = executable(&[0x66, 0x66, 0xcd, 0x80], 0x1000);
    let strict = decode_one(&memory, GuestAddress(0x1000)).unwrap_err();
    assert_eq!(
        decode_for_execution(&memory, GuestAddress(0x1000)).unwrap_err(),
        strict
    );
}
