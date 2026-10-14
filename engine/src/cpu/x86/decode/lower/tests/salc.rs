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
    let page = PageRange::new(GuestAddress(pc & !0xfff), 1).unwrap();
    memory.map_zeroed(page, Permissions::ALL).unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory.protect(page, Permissions::EXECUTE).unwrap();
    memory
}
#[test]
fn salc_is_one_flag_preserving_al_operation_in_both_code_defaults() {
    for default32 in [false, true] {
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
        let bytes = &[0xd6][..];
        let raw =
            Decoder::new(if default32 { 32 } else { 16 }, bytes, DecoderOptions::NONE).decode();
        assert_eq!(raw.code(), Code::Salc);
        assert_eq!(raw.len(), bytes.len());
        assert_eq!(raw.op_count(), 0);
        let memory = executable(bytes, 0x1000);
        for policy in [DecodePolicy::Strict, DecodePolicy::Execution] {
            let BoundDecode::Instruction(decoded) =
                decode_bound(&memory, GuestAddress(0x1000), cs, policy).unwrap()
            else {
                panic!("complete SALC")
            };
            assert_eq!(decoded.operation(), &Operation::SetAlFromCarry);
            assert_eq!(decoded.segment_accesses(), 0);
            assert_eq!(usize::from(decoded.length()), bytes.len());
            assert_eq!(decoded.next_pc(), GuestAddress(0x1000 + bytes.len() as u32));
            assert!(memory.is_code_current(decoded.code_snapshot()));
        }
    }
}
#[test]
fn salc_keeps_full_fetch_invalid_lock_and_repeat_profile_boundaries() {
    let bytes = [0xd6];
    let mut memory = executable(&[0x90], 0x1000);
    let missing = decode_one(&memory, GuestAddress(0x2000)).unwrap_err();
    assert!(matches!(
        missing,
        DecodeError::Paging {
            attempted_length: 1,
            error: crate::cpu::x86::CpuPagingError32::PageFault(_),
            ..
        }
    ));
    assert_eq!(
        decode_for_execution(&memory, GuestAddress(0x2000)).unwrap_err(),
        missing
    );
    let page = PageRange::new(GuestAddress(0x2000), 1).unwrap();
    memory.map_zeroed(page, Permissions::ALL).unwrap();
    memory.write(GuestAddress(0x2000), &bytes).unwrap();
    memory.protect(page, Permissions::READ).unwrap();
    let denied = decode_one(&memory, GuestAddress(0x2000)).unwrap_err();
    assert!(matches!(
        denied,
        DecodeError::Paging {
            attempted_length: 1,
            error: crate::cpu::x86::CpuPagingError32::PageFault(_),
            ..
        }
    ));
    assert_eq!(
        decode_for_execution(&memory, GuestAddress(0x2000)).unwrap_err(),
        denied
    );
    memory.protect(page, Permissions::EXECUTE).unwrap();
    assert_eq!(
        decode_for_execution(&memory, GuestAddress(0x2000))
            .unwrap()
            .operation(),
        &Operation::SetAlFromCarry
    );
    let locked = executable(&[0xf0, 0xd6], 0x1000);
    assert_eq!(
        decode_one(&locked, GuestAddress(0x1000)).unwrap_err(),
        DecodeError::InvalidEncoding
    );
    assert_eq!(
        decode_for_execution(&locked, GuestAddress(0x1000))
            .unwrap()
            .operation(),
        &Operation::Control(ControlOperation::InvalidOpcode)
    );
    for bytes in [
        &[0x66, 0xd6][..],
        &[0x67, 0xd6],
        &[0x64, 0xd6],
        &[0xf2, 0xd6],
        &[0xf3, 0xd6],
        &[0x66, 0x66, 0xd6],
    ] {
        let memory = executable(bytes, 0x1000);
        let strict = decode_one(&memory, GuestAddress(0x1000)).unwrap_err();
        assert!(matches!(strict, DecodeError::Unsupported(_)));
        assert_eq!(
            decode_for_execution(&memory, GuestAddress(0x1000)).unwrap_err(),
            strict
        );
    }
}
