use crate::{
    cpu::x86::{
        ControlOperation, SegmentCache, SegmentContext,
        capabilities::CPU_ID_LEAVES,
        decode::{
            BoundDecode, CodeFetchContext, DecodeError, DecodePolicy, decode_bound,
            decode_for_execution, decode_one,
        },
        ir::Operation,
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};
use iced_x86::{Code, CpuidFeature as F, Decoder, DecoderOptions};
fn rows() -> [(Code, &'static [u8]); 16] {
    [
        (Code::Monitorxd, &[0x0f, 0x01, 0xfa]),
        (Code::Monitorxw, &[0x67, 0x0f, 0x01, 0xfa]),
        (Code::Mwaitx, &[0x0f, 0x01, 0xfb]),
        (Code::Clzerod, &[0x0f, 0x01, 0xfc]),
        (Code::Clzerow, &[0x67, 0x0f, 0x01, 0xfc]),
        (Code::Rdpru, &[0x0f, 0x01, 0xfd]),
        (Code::XOP_Blcfill_r32_rm32, &[0x8f, 0xe9, 0x78, 0x01, 0xc8]),
        (Code::XOP_Blsfill_r32_rm32, &[0x8f, 0xe9, 0x78, 0x01, 0xd0]),
        (Code::XOP_Blcs_r32_rm32, &[0x8f, 0xe9, 0x78, 0x01, 0xd8]),
        (Code::XOP_Tzmsk_r32_rm32, &[0x8f, 0xe9, 0x78, 0x01, 0xe0]),
        (Code::XOP_Blcic_r32_rm32, &[0x8f, 0xe9, 0x78, 0x01, 0xe8]),
        (Code::XOP_Blsic_r32_rm32, &[0x8f, 0xe9, 0x78, 0x01, 0xf0]),
        (Code::XOP_T1mskc_r32_rm32, &[0x8f, 0xe9, 0x78, 0x01, 0xf8]),
        (Code::XOP_Blcmsk_r32_rm32, &[0x8f, 0xe9, 0x78, 0x02, 0xc8]),
        (Code::XOP_Blci_r32_rm32, &[0x8f, 0xe9, 0x78, 0x02, 0xf0]),
        (
            Code::XOP_Bextr_r32_rm32_imm32,
            &[0x8f, 0xea, 0x78, 0x10, 0xc1, 0x00, 0x00, 0x00, 0x00],
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
fn exact_miscellaneous_amd_bits_are_independent_of_media_and_supported_neighbors() {
    for (feature, function, register, bit) in [
        (F::TBM, 0x8000_0001, 2, 21),
        (F::MONITORX, 0x8000_0001, 2, 29),
        (F::CLZERO, 0x8000_0008, 1, 0),
        (F::RDPRU, 0x8000_0008, 1, 4),
    ] {
        assert_eq!(super::required(feature), Some((function, 0, register, bit)));
        assert!(!CPU_ID_LEAVES.iter().any(|leaf| leaf.function==function && leaf.registers[register] & (1<<bit)!=0));
    }
    let raw = Decoder::new(
        32,
        &[0x8f, 0xea, 0x78, 0x10, 0xc1, 0, 0, 0, 0],
        DecoderOptions::NONE,
    )
    .decode();
    assert_eq!(raw.cpuid_features(), &[F::TBM]);
    assert!(!raw.cpuid_features().contains(&F::XOP));
    assert_eq!(super::required(F::XOP), Some((0x8000_0001, 0, 2, 11)));
    for bytes in [
        &[0x0f, 0x77][..],
        &[0xc4, 0xe2, 0x79, 0x98, 0xc1],
        &[0xc4, 0xe2, 0x70, 0xf7, 0xc0],
    ] {
        let memory = executable(bytes, 0x1000);
        let strict = decode_one(&memory, GuestAddress(0x1000)).unwrap();
        assert_eq!(
            decode_for_execution(&memory, GuestAddress(0x1000))
                .unwrap()
                .operation(),
            strict.operation()
        );
    }
}
#[test]
fn selected_fault_codes_keep_both_code_defaults_and_ignore_poisoned_operand_context() {
    for default32 in [false, true] {
        for (code, bytes) in rows() {
            for segment_prefix in [&[][..], &[0x64]] {
                let mut body = bytes.to_vec();
                if !default32 {
                    if matches!(code, Code::Monitorxd | Code::Clzerod) {
                        body.insert(0, 0x67);
                    }
                    if matches!(code, Code::Monitorxw | Code::Clzerow) {
                        assert_eq!(body.remove(0), 0x67);
                    }
                }
                let full = [segment_prefix, &body].concat();
                let raw =
                    Decoder::new(if default32 { 32 } else { 16 }, &full, DecoderOptions::NONE)
                        .decode();
                assert_eq!(raw.code(), code, "{full:02x?}");
                assert_eq!(raw.len(), full.len());
                assert!(!raw.has_rep_prefix() && !raw.has_repne_prefix());
                let memory = executable(&full, 0x1000);
                let mut segments = SegmentContext::default();
                segments.state.selectors[3] = 0;
                segments.caches[3] = SegmentCache::from_bits(0).unwrap();
                segments.state.selectors[4] = 0;
                segments.caches[4] = SegmentCache::from_bits(0).unwrap();
                segments.caches[2] = SegmentCache::from_bits(0xf230_0fff).unwrap();
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
fn final_tbm_immediate_or_scalar_opcode_byte_must_be_executable_before_classification() {
    for bytes in [
        &[0x8f, 0xea, 0x78, 0x10, 0xc1, 0, 0, 0, 0][..],
        &[0x0f, 0x01, 0xfc],
    ] {
        let pc = 0x2000 - bytes.len() as u32 + 1;
        let mut memory = executable(&bytes[..bytes.len() - 1], pc);
        let missing = decode_one(&memory, GuestAddress(pc)).unwrap_err();
        assert!(
            matches!(missing,DecodeError::MemoryFault {length,..} if length==bytes.len() as u32)
        );
        assert_eq!(
            decode_for_execution(&memory, GuestAddress(pc)).unwrap_err(),
            missing
        );
        let page = PageRange::new(GuestAddress(0x2000), 1).unwrap();
        memory.map_zeroed(page, Permissions::ALL).unwrap();
        memory
            .write(GuestAddress(0x2000), &bytes[bytes.len() - 1..])
            .unwrap();
        memory.protect(page, Permissions::READ).unwrap();
        assert!(matches!(
            decode_for_execution(&memory, GuestAddress(pc)),
            Err(DecodeError::MemoryFault { .. })
        ));
        memory.protect(page, Permissions::EXECUTE).unwrap();
        assert_eq!(
            decode_for_execution(&memory, GuestAddress(pc))
                .unwrap()
                .operation(),
            &Operation::Control(ControlOperation::InvalidOpcode)
        );
    }
}
