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
use iced_x86::{Code, CpuidFeature, Decoder, DecoderOptions};
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
    for index in [3, 4] {
        context.state.selectors[index] = 0;
        context.caches[index] = SegmentCache::UNUSABLE;
    }
    context.caches[2] = SegmentCache::from_bits(0xf230_0fff).unwrap();
    if !default32 {
        context.caches[1] = SegmentCache::from_bits(0xf1bf_ffff).unwrap();
    }
    CodeFetchContext::from_segments(&context).unwrap()
}
use super::{available, unavailable};

#[test]
fn absent_nonserializing_msr_retains_strict_refusal_and_prepares_no_operand_ud_in_both_defaults() {
    assert!(!available((7, 1, 0, 19)));
    let bytes = [0x0f, 0x01, 0xc6];
    assert_eq!(USER_MODE_ENVIRONMENT.current_privilege(), 3);
    for default32 in [false, true] {
        let raw = Decoder::new(
            if default32 { 32 } else { 16 },
            &bytes,
            DecoderOptions::NONE,
        )
        .decode();
        assert_eq!(raw.code(), Code::Wrmsrns);
        assert_eq!(raw.cpuid_features(), &[CpuidFeature::WRMSRNS]);
        assert_eq!(raw.len(), bytes.len());
        assert!(!raw.has_rep_prefix() && !raw.has_repne_prefix());
        assert!(unavailable(&raw));
        let memory = executable(&bytes, 0x1000);
        let cs = fetch_context(default32);
        assert!(matches!(
            decode_bound(&memory, GuestAddress(0x1000), cs, DecodePolicy::Strict),
            Err(DecodeError::Unsupported(_))
        ));
        let BoundDecode::Instruction(decoded) =
            decode_bound(&memory, GuestAddress(0x1000), cs, DecodePolicy::Execution).unwrap()
        else {
            panic!("complete platform instruction");
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
#[test]
fn nonserializing_msr_last_byte_fetch_and_duplicate_or_lock_priority_stay_explicit() {
    let bytes = [0x0f, 0x01, 0xc6];
    let pc = 0x2001 - bytes.len() as u32;
    let mut memory = executable(&bytes[..bytes.len() - 1], pc);
    let missing = decode_one(&memory, GuestAddress(pc)).unwrap_err();
    assert!(
        matches!(missing,DecodeError::Paging { attempted_length: length, error: crate::cpu::x86::CpuPagingError32::PageFault(_), .. } if usize::from(length) == bytes.len())
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
    assert!(matches!(
        denied,
        DecodeError::Paging {
            error: crate::cpu::x86::CpuPagingError32::PageFault(_),
            ..
        }
    ));
    assert_eq!(
        decode_for_execution(&memory, GuestAddress(pc)).unwrap_err(),
        denied
    );
    memory.protect(page, Permissions::EXECUTE).unwrap();
    assert_eq!(
        decode_for_execution(&memory, GuestAddress(pc))
            .unwrap()
            .operation(),
        &Operation::Control(ControlOperation::InvalidOpcode)
    );
    let locked = [&[0xf0][..], &bytes].concat();
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

#[test]
fn ordinary_msr_privilege_and_unselected_list_form_remain_separate() {
    let raw = Decoder::new(32, &[0x0f, 0x30], DecoderOptions::NONE).decode();
    assert_eq!(raw.code(), Code::Wrmsr);
    assert!(!unavailable(&raw));
    let memory = executable(&[0x0f, 0x30], 0x1000);
    assert_eq!(
        decode_for_execution(&memory, GuestAddress(0x1000))
            .unwrap()
            .operation(),
        &Operation::Control(ControlOperation::GeneralProtectionZero)
    );
    let mut list = iced_x86::Instruction::default();
    list.set_code(Code::Wrmsrlist);
    assert!(!unavailable(&list));
    let duplicate = executable(&[0x66, 0x66, 0x0f, 0x01, 0xc6], 0x1000);
    assert_eq!(
        decode_one(&duplicate, GuestAddress(0x1000)).unwrap_err(),
        DecodeError::InvalidEncoding
    );
    assert_eq!(
        decode_for_execution(&duplicate, GuestAddress(0x1000))
            .unwrap()
            .operation(),
        &Operation::Control(ControlOperation::InvalidOpcode)
    );
}
