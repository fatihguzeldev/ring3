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
    for index in [0, 3, 4] {
        context.state.selectors[index] = 0;
        context.caches[index] = SegmentCache::UNUSABLE;
    }
    context.caches[2] = SegmentCache::from_bits(0xf230_0fff).unwrap();
    if !default32 {
        context.caches[1] = SegmentCache::from_bits(0xf1bf_ffff).unwrap();
    }
    CodeFetchContext::from_segments(&context).unwrap()
}

#[test]
fn absent_history_reset_precedes_enabled_eax_and_privilege_branches_in_both_defaults() {
    assert!(!available((7, 1, 0, 22)));
    assert_eq!(USER_MODE_ENVIRONMENT.current_privilege(), 3);
    for default32 in [false, true] {
        for immediate in [0, 0xff] {
            let bytes = [0xf3, 0x0f, 0x3a, 0xf0, 0xc0, immediate];
            let raw = Decoder::new(
                if default32 { 32 } else { 16 },
                &bytes,
                DecoderOptions::NONE,
            )
            .decode();
            assert_eq!(raw.code(), Code::Hreset_imm8);
            assert_eq!(raw.cpuid_features(), &[CpuidFeature::HRESET]);
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
                panic!("complete history instruction");
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
    let raw = Decoder::new(32, &[0x90], DecoderOptions::NONE).decode();
    assert!(!unavailable(&raw));
    assert_eq!(
        decode_one(&executable(&[0x90], 0x1000), GuestAddress(0x1000))
            .unwrap()
            .operation(),
        &Operation::Nop
    );
}

#[test]
fn history_immediate_fetch_duplicate_prefix_and_invalid_lock_remain_distinct() {
    let bytes = [0xf3, 0x0f, 0x3a, 0xf0, 0xc0, 0xff];
    let pc = 0x2000 - bytes.len() as u32 + 1;
    let mut memory = executable(&bytes[..bytes.len() - 1], pc);
    let missing = decode_one(&memory, GuestAddress(pc)).unwrap_err();
    assert!(
        matches!(missing, DecodeError::MemoryFault { length, .. } if length == bytes.len() as u32)
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
    assert_eq!(
        decode_for_execution(&memory, GuestAddress(pc))
            .unwrap()
            .operation(),
        &Operation::Control(ControlOperation::InvalidOpcode)
    );
    let duplicate = executable(&[0xf3, 0xf3, 0x0f, 0x3a, 0xf0, 0xc0, 0], 0x1000);
    let strict = decode_one(&duplicate, GuestAddress(0x1000)).unwrap_err();
    assert!(matches!(strict, DecodeError::Unsupported(_)));
    assert_eq!(
        decode_for_execution(&duplicate, GuestAddress(0x1000)).unwrap_err(),
        strict
    );
    let locked = executable(&[0xf0, 0xf3, 0x0f, 0x3a, 0xf0, 0xc0, 0], 0x1000);
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
}
