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
fn exact_absent_instruction_prefetch_forms_are_nops_without_operand_authority() {
    for default32 in [false, true] {
        for (code, modrm) in [(Code::Prefetchit1_m8, 0x30), (Code::Prefetchit0_m8, 0x38)] {
            for prefix in [&[][..], &[0x64, 0x67]] {
                let bytes = [prefix, &[0x0f, 0x18, modrm]].concat();
                let raw = Decoder::new(
                    if default32 { 32 } else { 16 },
                    &bytes,
                    DecoderOptions::NONE,
                )
                .decode();
                assert_eq!(raw.code(), code);
                assert_eq!(raw.len(), bytes.len());
                assert!(super::ignored_instruction_prefetch(&raw));
                let memory = executable(&bytes, 0x1000);
                let mut segments = SegmentContext::default();
                segments.state.selectors[3] = 0;
                segments.caches[3] = SegmentCache::from_bits(0).unwrap();
                segments.state.selectors[4] = 0;
                segments.caches[4] = SegmentCache::from_bits(0).unwrap();
                if !default32 {
                    segments.caches[1] = SegmentCache::from_bits(0xf1bf_ffff).unwrap();
                }
                let cs = CodeFetchContext::from_segments(&segments).unwrap();
                for policy in [DecodePolicy::Strict, DecodePolicy::Execution] {
                    let BoundDecode::Instruction(decoded) =
                        decode_bound(&memory, GuestAddress(0x1000), cs, policy).unwrap()
                    else {
                        panic!("complete code")
                    };
                    assert_eq!(decoded.operation(), &Operation::Nop);
                    assert_eq!(decoded.segment_accesses(), 0);
                    assert_eq!(usize::from(decoded.length()), bytes.len());
                    assert!(memory.is_code_current(decoded.code_snapshot()));
                }
            }
        }
    }
}
#[test]
fn instruction_prefetch_retains_prefix_neighbor_and_complete_fetch_boundaries() {
    for modrm in [0x30, 0x38] {
        let bytes = [0x0f, 0x18, modrm];
        let pc = 0x1ffe;
        let memory = executable(&bytes[..2], pc);
        let missing = decode_one(&memory, GuestAddress(pc)).unwrap_err();
        assert!(matches!(
            missing,
            DecodeError::MemoryFault { length: 3, .. }
        ));
        assert_eq!(
            decode_for_execution(&memory, GuestAddress(pc)).unwrap_err(),
            missing
        );
        let locked = [0xf0, 0x0f, 0x18, modrm];
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
        for prefix in [&[0xf3][..], &[0x66, 0x66]] {
            let encoded = [prefix, &bytes].concat();
            let memory = executable(&encoded, 0x1000);
            let strict = decode_one(&memory, GuestAddress(0x1000)).unwrap_err();
            assert!(matches!(strict, DecodeError::Unsupported(_)));
            assert_eq!(
                decode_for_execution(&memory, GuestAddress(0x1000)).unwrap_err(),
                strict
            );
        }
    }
    let neighbor = [0x0f, 0x19, 0];
    let raw = Decoder::new(32, &neighbor, DecoderOptions::NONE).decode();
    assert!(!super::ignored_instruction_prefetch(&raw));
    let memory = executable(&neighbor, 0x1000);
    let strict = decode_one(&memory, GuestAddress(0x1000)).unwrap_err();
    assert_eq!(
        decode_for_execution(&memory, GuestAddress(0x1000)).unwrap_err(),
        strict
    );
}
