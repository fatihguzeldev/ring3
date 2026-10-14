use crate::{
    cpu::x86::{
        ControlOperation, SegmentCache, SegmentContext,
        decode::{
            BoundDecode, CodeFetchContext, DecodeError, DecodePolicy, decode_bound,
            decode_for_execution, decode_one,
        },
        environment::USER_MODE_ENVIRONMENT,
        ir::Operation,
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};
use iced_x86::{Code, Decoder, DecoderOptions};

fn rows() -> [(Code, &'static [u8], ControlOperation); 9] {
    use ControlOperation::{GeneralProtectionZero as Gp, InvalidOpcode as Ud};
    [
        (Code::XOP_Llwpcb_r32, &[0x8f, 0xe9, 0x78, 0x12, 0xc0], Ud),
        (Code::XOP_Slwpcb_r32, &[0x8f, 0xe9, 0x78, 0x12, 0xc8], Ud),
        (
            Code::XOP_Lwpins_r32_rm32_imm32,
            &[0x8f, 0xea, 0x78, 0x12, 0xc1, 0, 0, 0, 0],
            Ud,
        ),
        (
            Code::XOP_Lwpval_r32_rm32_imm32,
            &[0x64, 0x8f, 0xea, 0x78, 0x12, 0x08, 0, 0, 0, 0],
            Ud,
        ),
        (Code::Pconfig, &[0x0f, 0x01, 0xc5], Ud),
        (Code::Invlpgbd, &[0x0f, 0x01, 0xfe], Ud),
        (Code::Invlpgbw, &[0x67, 0x0f, 0x01, 0xfe], Ud),
        (Code::Tlbsync, &[0x0f, 0x01, 0xff], Ud),
        (Code::Wbnoinvd, &[0xf3, 0x0f, 0x09], Gp),
    ]
}
fn executable(bytes: &[u8], pc: u32) -> AddressSpace {
    let mut memory = AddressSpace::new(2).unwrap();
    let page = PageRange::new(GuestAddress(pc & !0xfff), 1).unwrap();
    memory.map_zeroed(page, Permissions::ALL).unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory.protect(page, Permissions::EXECUTE).unwrap();
    memory
}
fn fetch_context(default32: bool) -> CodeFetchContext {
    let mut context = SegmentContext::default();
    context.state.selectors[4] = 0;
    context.caches[4] = SegmentCache::from_bits(0).unwrap();
    if !default32 {
        context.caches[1] = SegmentCache::from_bits(0xf1bfffff).unwrap();
    }
    CodeFetchContext::from_segments(&context).unwrap()
}
#[test]
fn qualified_system_faults_keep_strict_decode_and_precede_operand_lowering_in_both_defaults() {
    for (expected_code, bytes, operation) in rows() {
        let memory = executable(bytes, 0x1000);
        assert!(matches!(
            decode_one(&memory, GuestAddress(0x1000)),
            Err(DecodeError::Unsupported(_))
        ));
        for default32 in [false, true] {
            let raw =
                Decoder::new(if default32 { 32 } else { 16 }, bytes, DecoderOptions::NONE).decode();
            let code = match (expected_code, default32) {
                (Code::Invlpgbd, false) => Code::Invlpgbw,
                (Code::Invlpgbw, false) => Code::Invlpgbd,
                _ => expected_code,
            };
            assert_eq!(raw.code(), code);
            assert_eq!(raw.len(), bytes.len());
            assert!(!raw.has_rep_prefix() && !raw.has_repne_prefix());
            assert!(matches!(
                decode_bound(
                    &memory,
                    GuestAddress(0x1000),
                    fetch_context(default32),
                    DecodePolicy::Strict
                ),
                Err(DecodeError::Unsupported(_))
            ));
            let BoundDecode::Instruction(decoded) = decode_bound(
                &memory,
                GuestAddress(0x1000),
                fetch_context(default32),
                DecodePolicy::Execution,
            )
            .unwrap() else {
                panic!("unexpected code fault")
            };
            assert_eq!(decoded.operation(), &Operation::Control(operation));
            assert_eq!(decoded.segment_accesses(), 0);
            assert_eq!(usize::from(decoded.length()), bytes.len());
            assert_eq!(decoded.next_pc(), GuestAddress(0x1000 + bytes.len() as u32));
            assert!(memory.is_code_current(decoded.code_snapshot()));
        }
    }
}
#[test]
fn privilege_rules_do_not_map_wbnoinvd_or_pconfig_through_feature_absence() {
    let wb = Decoder::new(32, &[0xf3, 0x0f, 0x09], DecoderOptions::NONE).decode();
    assert!(!super::optional::unavailable(&wb));
    assert_eq!(
        super::for_instruction(&wb, &[0xf3, 0x0f, 0x09], USER_MODE_ENVIRONMENT),
        Some(ControlOperation::GeneralProtectionZero)
    );
    let pc = Decoder::new(32, &[0x0f, 0x01, 0xc5], DecoderOptions::NONE).decode();
    assert!(super::user_privilege_invalid(&pc, USER_MODE_ENVIRONMENT));
}
#[test]
fn complete_instruction_fetch_still_precedes_system_fault_classification() {
    for bytes in [
        &[0x8f, 0xea, 0x78, 0x12, 0xc1, 0, 0, 0, 0][..],
        &[0xf3, 0x0f, 0x09],
    ] {
        let pc = 0x2000 - bytes.len() as u32 + 1;
        let mut memory = executable(&bytes[..bytes.len() - 1], pc);
        let missing = decode_one(&memory, GuestAddress(pc)).unwrap_err();
        assert!(
            matches!(missing, DecodeError::MemoryFault {length, ..} if length == bytes.len() as u32)
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
        let denied = decode_one(&memory, GuestAddress(pc)).unwrap_err();
        assert!(matches!(denied, DecodeError::MemoryFault { .. }));
        assert_eq!(
            decode_for_execution(&memory, GuestAddress(pc)).unwrap_err(),
            denied
        );
        memory.protect(page, Permissions::EXECUTE).unwrap();
        let decoded = decode_for_execution(&memory, GuestAddress(pc)).unwrap();
        assert_eq!(
            decoded.operation(),
            &Operation::Control(if bytes[0] == 0xf3 {
                ControlOperation::GeneralProtectionZero
            } else {
                ControlOperation::InvalidOpcode
            })
        );
    }
}
#[test]
fn prefix_refusals_and_separately_qualified_tbm_fault_stay_distinct() {
    for bytes in [
        &[0xf3, 0xf3, 0x0f, 0x09][..],
        &[0xf2, 0x0f, 0x09],
        &[0x8f, 0xea, 0x78, 0x10, 0xc1, 0, 0, 0, 0],
    ] {
        let memory = executable(bytes, 0x1000);
        let strict = decode_one(&memory, GuestAddress(0x1000)).unwrap_err();
        if bytes == [0x8f, 0xea, 0x78, 0x10, 0xc1, 0, 0, 0, 0] {
            assert_eq!(
                decode_for_execution(&memory, GuestAddress(0x1000))
                    .unwrap()
                    .operation(),
                &Operation::Control(ControlOperation::InvalidOpcode)
            );
        } else {
            assert_eq!(
                decode_for_execution(&memory, GuestAddress(0x1000)).unwrap_err(),
                strict
            );
        }
    }
    for bytes in [&[0xf0, 0xf3, 0x0f, 0x09][..], &[0x66, 0x0f, 0x01, 0xc5]] {
        let memory = executable(bytes, 0x1000);
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
    for bytes in [&[0x0f, 0x09][..], &[0x66, 0x0f, 0x09], &[0x0f, 0x08]] {
        let memory = executable(bytes, 0x1000);
        assert_eq!(
            decode_for_execution(&memory, GuestAddress(0x1000))
                .unwrap()
                .operation(),
            &Operation::Control(ControlOperation::GeneralProtectionZero)
        );
    }
}
