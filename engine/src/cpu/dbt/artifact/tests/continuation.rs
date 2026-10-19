use super::{group_tests::resident, *};
use crate::{
    cpu::x86::{SegmentContext, decode::DecodeAuthority},
    memory::{PageRange, Permissions},
};

fn memory(bytes: &[u8]) -> AddressSpace {
    let mut memory = AddressSpace::new(2).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 2).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    memory
}

fn prepared(memory: &AddressSpace, length: usize) -> PreparedRegion {
    prepare_embedded_region(
        memory,
        &[BlockSpec {
            entry: GuestAddress(0x1000),
            byte_length: u32::try_from(length).unwrap(),
        }],
        CompileLimits::default(),
        &[],
        CodeFetchContext::from_segments(&SegmentContext::default()),
        DecodeAuthority::default(),
    )
    .unwrap()
}

fn certificate(bytes: &[u8]) -> bool {
    let memory = memory(bytes);
    CompiledRegion::new(
        prepared(&memory, bytes.len()),
        Vec::new(),
        super::super::CompilationTier32::Baseline,
        resident(),
    )
    .unwrap()
    .preserves_extended_record_validity()
}

#[test]
fn final_ir_certifies_scalar_memory_native_x87_and_inline_fp_writers() {
    for bytes in [
        &[0x90][..], // nop
        &[0xb8, 0, 0x20, 0, 0, 0x89, 0x00, 0x8b, 0x08, 0x83, 0xc1, 1][..],
        &[0x50, 0x58, 0xc3][..],                   // push eax; pop eax; ret
        &[0xe8, 0, 0, 0, 0][..],                   // near call to following instruction
        &[0xdb, 0xe3, 0xdb, 0xe2, 0xdf, 0xe0][..], // fninit; fnclex; fnstsw ax
        &[0xdd, 0x3d, 0, 0x20, 0, 0, 0xd9, 0x3d, 0, 0x20, 0, 0][..],
        &[
            0xd9, 0x05, 0, 0x20, 0, 0, 0xd8, 0xc0, 0xd9, 0x1d, 4, 0x20, 0, 0,
        ][..],
        &[0xd9, 0x2d, 0, 0x20, 0, 0, 0xd9, 0xfa, 0xd9, 0xf1][..], // fldcw; fsqrt; fyl2x
    ] {
        assert!(certificate(bytes), "{bytes:02x?}");
    }
    for length in [1, 2, 4, 8, 9, 32] {
        let memory = memory(&[0xd9, 0xe8].repeat(length));
        for tier in [
            super::super::CompilationTier32::Baseline,
            super::super::CompilationTier32::Optimized,
        ] {
            let region =
                CompiledRegion::new(prepared(&memory, length * 2), Vec::new(), tier, resident())
                    .unwrap();
            assert!(region.preserves_extended_record_validity());
            if length == 2 {
                assert!(region.native_x87_pair(0x1000).is_some());
            }
            if length == 4 {
                assert!(region.native_x87_group(0).is_some());
            }
            if length == 32 {
                for ordinal in 0..4 {
                    let legacy = region.native_x87_group(ordinal).unwrap();
                    assert_eq!(legacy.steps().len(), 8);
                    assert_eq!(legacy.steps()[0].pc, 0x1000 + ordinal * 16);
                    assert!(region.native_x87_group32(ordinal).is_none());
                }
                let wide = region.native_x87_group32(4).unwrap();
                assert_eq!(wide.steps().len(), 32);
                assert_eq!(wide.steps()[0].pc, 0x1000);
                assert_eq!(wide.steps()[31].pc, 0x103e);
                assert!(region.native_x87_group32(5).is_none());
            }
        }
    }
}

#[test]
fn unreviewed_writers_and_any_unsupported_retained_block_fail_closed() {
    for bytes in [
        &[0x0f, 0x57, 0xc0][..],          // xorps xmm0,xmm0
        &[0x0f, 0x77][..],                // emms
        &[0x0f, 0xae, 0x00][..],          // fxsave [eax]
        &[0x8e, 0xd8][..],                // mov ds,ax
        &[0xa4][..],                      // movsb
        &[0x0f, 0xa2][..],                // cpuid: deliberately not certified in v1
        &[0xd9, 0x25, 0, 0x20, 0, 0][..], // fldenv
        &[0xd9, 0x35, 0, 0x20, 0, 0][..], // fnstenv
        &[0xdd, 0x25, 0, 0x20, 0, 0][..], // frstor
        &[0xdd, 0x35, 0, 0x20, 0, 0][..], // fnsave
    ] {
        assert!(!certificate(bytes), "{bytes:02x?}");
    }
    let memory = memory(&[0x90, 0x0f, 0x57, 0xc0]);
    let mut first = prepared(&memory, 1);
    first.blocks.append(&mut prepared(&memory, 4).blocks);
    let region = CompiledRegion::new(
        first,
        Vec::new(),
        super::super::CompilationTier32::Baseline,
        resident(),
    )
    .unwrap();
    assert!(!region.preserves_extended_record_validity());

    let gate_memory = self::memory(&[0x0f, 0x0b]);
    let gate = prepare_embedded_region(
        &gate_memory,
        &[BlockSpec {
            entry: GuestAddress(0x1000),
            byte_length: 2,
        }],
        CompileLimits::default(),
        &[GateSpec {
            entry: GuestAddress(0x1000),
            id: 17,
        }],
        CodeFetchContext::from_segments(&SegmentContext::default()),
        DecodeAuthority::default(),
    )
    .unwrap();
    let region = CompiledRegion::new(
        gate,
        Vec::new(),
        super::super::CompilationTier32::Baseline,
        resident(),
    )
    .unwrap();
    assert!(region.matches_gate(0x1000, 17));
    assert!(!region.preserves_extended_record_validity());
}

#[test]
fn certificate_is_recomputed_from_the_retained_byte_fit_prefix() {
    let memory = memory(&[0x90, 0x0f, 0xa2]);
    let minimum = prepared(&memory, 1);
    let budget = wasm::emit(&minimum.blocks, resident(), minimum.decode_key).len();
    let full = prepared(&memory, 3);
    assert!(!continuation::preserves_extended_record_validity(&full));
    let region = emit_entry_prepared(
        &memory,
        full,
        CompileLimits {
            wasm_bytes: budget,
            ..CompileLimits::default()
        },
        resident(),
        CodeFetchContext::from_segments(&SegmentContext::default()),
    )
    .unwrap();
    assert_eq!(region.metadata().instructions, 1);
    assert!(region.preserves_extended_record_validity());

    let cs = CodeFetchContext::from_segments(&SegmentContext::default()).unwrap();
    let fault = compile_runtime_entry(
        &memory,
        GuestAddress(0x3000),
        CompileLimits::default(),
        7,
        1,
        cs,
        DecodeAuthority::default(),
    )
    .unwrap();
    assert!(!fault.preserves_extended_record_validity());
}
