use crate::{
    cpu::x86::{
        ControlOperation, SegmentCache, SegmentContext,
        decode::{
            BoundDecode, CodeFetchContext, DecodeError, DecodePolicy, decode_bound,
            decode_for_execution, decode_one,
        },
        ir::Operation,
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};
use iced_x86::{Code, Decoder, DecoderOptions};

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
fn equal_width_word_encodings_keep_original_length_and_canonical_move_semantics() {
    for (opcode, code) in [(0xb7, Code::Movzx_r16_rm16), (0xbf, Code::Movsx_r16_rm16)] {
        assert_eq!(super::canonical_code(code), Some(Code::Mov_r16_rm16));
        for default32 in [false, true] {
            for memory in [false, true] {
                let mut raw = if default32 { vec![0x66] } else { vec![] };
                let mut canonical = raw.clone();
                raw.extend([0x0f, opcode, if memory { 0x03 } else { 0xc1 }]);
                canonical.extend([0x8b, if memory { 0x03 } else { 0xc1 }]);
                if memory {
                    raw.splice(0..0, [0x64, 0x67]);
                    canonical.splice(0..0, [0x64, 0x67]);
                }
                let decoded = Decoder::with_ip(
                    if default32 { 32 } else { 16 },
                    &raw,
                    0x1000,
                    DecoderOptions::NONE,
                )
                .decode();
                assert_eq!(decoded.code(), code);
                assert_eq!(decoded.len(), raw.len());
                let mut normalized = decoded;
                normalized.set_code(super::canonical_code(code).unwrap());
                assert_eq!(normalized.len(), decoded.len());
                assert_eq!(normalized.next_ip(), decoded.next_ip());
                normalized.set_code(code);
                assert!(normalized.eq_all_bits(&decoded));
                let original_memory = executable(&raw, 0x1000);
                let canonical_memory = executable(&canonical, 0x1000);
                let mut context = SegmentContext::default();
                if !default32 {
                    context.caches[1] = SegmentCache::from_bits(0xf1bf_ffff).unwrap();
                }
                let cs = CodeFetchContext::from_segments(&context).unwrap();
                for policy in [DecodePolicy::Strict, DecodePolicy::Execution] {
                    let BoundDecode::Instruction(actual) =
                        decode_bound(&original_memory, GuestAddress(0x1000), cs, policy).unwrap()
                    else {
                        panic!("complete equal-width move");
                    };
                    let BoundDecode::Instruction(want) =
                        decode_bound(&canonical_memory, GuestAddress(0x1000), cs, policy).unwrap()
                    else {
                        panic!("complete canonical word move");
                    };
                    assert_eq!(actual.operation(), want.operation());
                    assert!(matches!(
                        actual.operation(),
                        Operation::MoveWord { .. } | Operation::LoadWord { .. }
                    ));
                    assert_eq!(actual.segment_accesses(), want.segment_accesses());
                    assert_eq!(usize::from(actual.length()), raw.len());
                    assert_eq!(actual.next_pc(), GuestAddress(0x1000 + raw.len() as u32));
                    assert_eq!(usize::from(want.length()) + 1, usize::from(actual.length()));
                    assert!(original_memory.is_code_current(actual.code_snapshot()));
                    let mut observed = vec![0; raw.len()];
                    original_memory
                        .read(GuestAddress(0x1000), &mut observed)
                        .unwrap();
                    assert_eq!(observed, raw);
                }
            }
        }
    }
    for code in [
        Code::Movzx_r32_rm16,
        Code::Movsx_r32_rm16,
        Code::Movzx_r16_rm8,
        Code::Movsx_r16_rm8,
    ] {
        assert_eq!(
            super::canonical_code(code),
            None,
            "existing widening stays distinct"
        );
    }
}

#[test]
fn original_extra_opcode_byte_and_modrm_must_be_fetched_and_tracked() {
    for opcode in [0xb7, 0xbf] {
        let bytes = [0x66, 0x0f, opcode, 0xc1];
        let pc = 0x2001 - bytes.len() as u32;
        let mut memory = executable(&bytes[..bytes.len() - 1], pc);
        assert!(
            matches!(decode_one(&memory,GuestAddress(pc)),Err(DecodeError::Paging { attempted_length: length, error: crate::cpu::x86::CpuPagingError32::PageFault(_), .. }) if usize::from(length) == bytes.len())
        );
        let page = PageRange::new(GuestAddress(0x2000), 1).unwrap();
        memory.map_zeroed(page, Permissions::ALL).unwrap();
        memory
            .write(GuestAddress(0x2000), &bytes[bytes.len() - 1..])
            .unwrap();
        memory.protect(page, Permissions::READ).unwrap();
        assert!(matches!(
            decode_for_execution(&memory, GuestAddress(pc)),
            Err(DecodeError::Paging {
                error: crate::cpu::x86::CpuPagingError32::PageFault(_),
                ..
            })
        ));
        memory.protect(page, Permissions::ALL).unwrap();
        let decoded = decode_one(&memory, GuestAddress(pc)).unwrap();
        assert_eq!(usize::from(decoded.length()), bytes.len());
        assert_eq!(decoded.next_pc(), GuestAddress(0x2001));
        assert_eq!(decoded.code_snapshot().versions.len(), 2);
        assert!(memory.is_code_current(decoded.code_snapshot()));
        memory.write(GuestAddress(0x2000), &[0xc2]).unwrap();
        assert!(!memory.is_code_current(decoded.code_snapshot()));
    }
}

#[test]
fn equal_word_move_prefix_faults_and_other_unqualified_encodings_stay_distinct() {
    for opcode in [0xb7, 0xbf] {
        let memory = executable(&[0x66, 0x66, 0x0f, opcode, 0xc1], 0x1000);
        let strict = decode_one(&memory, GuestAddress(0x1000)).unwrap_err();
        assert!(matches!(strict, DecodeError::Unsupported(_)));
        assert_eq!(
            decode_for_execution(&memory, GuestAddress(0x1000)).unwrap_err(),
            strict
        );
        let memory = executable(&[0xf0, 0x66, 0x0f, opcode, 0xc1], 0x1000);
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
    for code in [
        Code::Test_rm8_imm8_F6r1,
        Code::Test_rm16_imm16_F7r1,
        Code::Test_rm32_imm32_F7r1,
        Code::Bswap_r16,
    ] {
        assert_eq!(super::canonical_code(code), None);
    }
}
