use super::{available, unavailable};
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
use iced_x86::{Code, CpuidFeature, Decoder, DecoderOptions, Instruction};

fn executable(bytes: &[u8], pc: u32) -> AddressSpace {
    let mut memory = AddressSpace::new(2).unwrap();
    let page = PageRange::new(GuestAddress(pc & !0xfff), 1).unwrap();
    memory.map_zeroed(page, Permissions::ALL).unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory.protect(page, Permissions::EXECUTE).unwrap();
    memory
}

#[test]
fn exact_absent_invpcid_rule_precedes_operand_and_privilege_work_in_both_defaults() {
    assert!(!available((7, 0, 1, 10)));
    assert_eq!(USER_MODE_ENVIRONMENT.current_privilege(), 3);
    for default32 in [false, true] {
        for body in [
            &[0x66, 0x0f, 0x38, 0x82, 0x08][..],
            &[0x36, 0x66, 0x0f, 0x38, 0x82, 0x0c, 0x24],
            &[0x64, 0x67, 0x66, 0x0f, 0x38, 0x82, 0x08],
        ] {
            let mut bytes = body.to_vec();
            // the SIB form keeps address32 independently of the code default.
            if !default32 && bytes[0] == 0x36 {
                bytes.insert(0, 0x67);
            }
            let raw = Decoder::new(
                if default32 { 32 } else { 16 },
                &bytes,
                DecoderOptions::NONE,
            )
            .decode();
            assert_eq!(raw.code(), Code::Invpcid_r32_m128);
            assert_eq!(raw.len(), bytes.len());
            assert_eq!(raw.cpuid_features(), &[CpuidFeature::INVPCID]);
            assert!(!raw.has_rep_prefix() && !raw.has_repne_prefix());
            assert!(unavailable(&raw));
            let memory = executable(&bytes, 0x1000);
            let mut context = SegmentContext::default();
            for index in [3, 4] {
                context.state.selectors[index] = 0;
                context.caches[index] = SegmentCache::UNUSABLE;
            }
            context.caches[2] = SegmentCache::from_bits(0xf230_0fff).unwrap();
            if !default32 {
                context.caches[1] = SegmentCache::from_bits(0xf1bf_ffff).unwrap();
            }
            let cs = CodeFetchContext::from_segments(&context).unwrap();
            assert!(matches!(
                decode_bound(&memory, GuestAddress(0x1000), cs, DecodePolicy::Strict),
                Err(DecodeError::Unsupported(_))
            ));
            let BoundDecode::Instruction(decoded) =
                decode_bound(&memory, GuestAddress(0x1000), cs, DecodePolicy::Execution).unwrap()
            else {
                panic!("complete executable instruction");
            };
            assert_eq!(
                decoded.operation(),
                &Operation::Control(ControlOperation::InvalidOpcode)
            );
            assert_eq!(decoded.segment_accesses(), 0);
            assert_eq!(usize::from(decoded.length()), bytes.len());
            assert_eq!(decoded.next_pc(), GuestAddress(0x1000 + bytes.len() as u32));
            assert!(memory.is_code_current(decoded.code_snapshot()));
        }
    }
    for code in [Code::Invpcid_r64_m128, Code::Invlpg_m] {
        let mut instruction = Instruction::default();
        instruction.set_code(code);
        assert!(
            !unavailable(&instruction),
            "the selected rule is exact32bit INVPCID"
        );
    }
}

#[test]
fn invpcid_final_memory_modrm_must_be_executable_before_absence_classification() {
    let bytes = [0x66, 0x0f, 0x38, 0x82, 0x08];
    let pc = 0x2000 - bytes.len() as u32 + 1;
    let mut memory = executable(&bytes[..bytes.len() - 1], pc);
    let missing = decode_one(&memory, GuestAddress(pc)).unwrap_err();
    assert!(matches!(missing,DecodeError::MemoryFault {length,..} if length==bytes.len() as u32));
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
        &Operation::Control(ControlOperation::InvalidOpcode)
    );
    assert_eq!(decoded.segment_accesses(), 0);
}

#[test]
fn invpcid_duplicate_prefix_and_invalid_encodings_keep_their_existing_boundaries() {
    let bytes = [0x66, 0x66, 0x0f, 0x38, 0x82, 0x08];
    let raw = Decoder::new(32, &bytes, DecoderOptions::NONE).decode();
    assert_eq!(raw.code(), Code::Invpcid_r32_m128);
    let memory = executable(&bytes, 0x1000);
    let strict = decode_one(&memory, GuestAddress(0x1000)).unwrap_err();
    assert!(matches!(strict, DecodeError::Unsupported(_)));
    assert_eq!(
        decode_for_execution(&memory, GuestAddress(0x1000)).unwrap_err(),
        strict
    );
    for bytes in [
        &[0xf0, 0x66, 0x0f, 0x38, 0x82, 0x08][..],
        &[0x66, 0x0f, 0x38, 0x82, 0xc1],
        &[0xf2, 0x66, 0x0f, 0x38, 0x82, 0x08],
    ] {
        let raw = Decoder::new(32, bytes, DecoderOptions::NONE).decode();
        assert_eq!(raw.code(), Code::INVALID);
        assert_eq!(raw.len(), bytes.len());
        let memory = executable(bytes, 0x1000);
        assert_eq!(
            decode_one(&memory, GuestAddress(0x1000)).unwrap_err(),
            DecodeError::InvalidEncoding
        );
        let decoded = decode_for_execution(&memory, GuestAddress(0x1000)).unwrap();
        assert_eq!(
            decoded.operation(),
            &Operation::Control(ControlOperation::InvalidOpcode)
        );
        assert_eq!(decoded.segment_accesses(), 0);
    }
    let memory = executable(&[0x0f, 0x01, 0x38], 0x1000);
    assert_eq!(
        decode_for_execution(&memory, GuestAddress(0x1000))
            .unwrap()
            .operation(),
        &Operation::Control(ControlOperation::GeneralProtectionZero)
    );
}
