use super::{
    group_tests::{memory, prepared, resident},
    *,
};
use crate::cpu::x86::SegmentRegister;

#[test]
fn native_plans_share_final_ir_steps_and_pin_only_immutable_metadata() {
    let memory = memory();
    let mut input = prepared(&memory, 0x1000, 32);
    input
        .blocks
        .append(&mut prepared(&memory, 0x1000, 9).blocks);
    let region = CompiledRegion::new(
        input,
        Vec::new(),
        super::super::CompilationTier32::Baseline,
        resident(),
    )
    .unwrap();
    let wide = region.native_x87_group32(5).unwrap();
    let overlap = region.native_x87_group32(6).unwrap();
    let legacy = region.native_x87_group(0).unwrap();
    let pair = region.native_x87_pair(0x1000).unwrap();
    assert_eq!(wide.steps().len(), 32);
    assert_eq!(overlap.steps().len(), 9);
    assert_eq!(legacy.steps().len(), 8);
    assert_eq!(pair.steps().len(), 2);
    assert_eq!(wide.steps().as_ptr(), legacy.steps().as_ptr());
    assert_eq!(
        pair.steps().as_ptr(),
        region.native_x87_pair(0x1000).unwrap().steps().as_ptr()
    );
    assert_ne!(wide.steps().as_ptr(), overlap.steps().as_ptr());
    assert!(region.native_x87_group32(0).is_none());
    assert!(region.native_x87_group(5).is_none());
    assert!(region.native_x87_pair(0x1001).is_none());
    assert!(region.native_x87_group32(u32::MAX).is_none());
    for (index, step) in wide.steps().iter().enumerate() {
        let descriptor = region.x87_group32(5).unwrap().descriptor(index).unwrap();
        assert_eq!(step.operation, descriptor.operation);
        assert_eq!(step.pc, descriptor.pc);
        assert_eq!(step.instruction_length, descriptor.length);
        assert_eq!(step.opcode, descriptor.opcode);
        assert_eq!(step.segment, SegmentRegister::Ds);
        assert_eq!(step.read_size, 0);
        assert_eq!(step.write_size, 0);
        assert!(step.waiting);
    }
    drop(region);
    assert_eq!(wide.steps().last().unwrap().pc, 0x103e);
    assert_eq!(overlap.steps().last().unwrap().pc, 0x1010);
}

#[test]
fn native_plans_are_resident_only_and_follow_the_final_byte_fit_prefix() {
    let memory = memory();
    for binding in [
        None,
        Some(wasm::EmbeddedBinding::Replacement {
            key: 7,
            generation: 1,
        }),
    ] {
        let region = CompiledRegion::new(
            prepared(&memory, 0x1000, 32),
            Vec::new(),
            super::super::CompilationTier32::Baseline,
            binding,
        )
        .unwrap();
        assert!(region.native_x87_pair(0x1000).is_none());
        assert!(region.native_x87_group(0).is_none());
        assert!(region.native_x87_group32(0).is_none());
    }
    let minimum = prepared(&memory, 0x1000, 1);
    let budget = wasm::emit(&minimum.blocks, resident(), minimum.decode_key).len();
    let region = emit_entry_prepared(
        &memory,
        prepared(&memory, 0x1000, 32),
        CompileLimits {
            wasm_bytes: budget,
            ..CompileLimits::default()
        },
        resident(),
        CodeFetchContext::from_segments(&crate::cpu::x86::SegmentContext::default()),
    )
    .unwrap();
    assert!(region.metadata().instructions < 9);
    for ordinal in 0..=region.x87_groups.len() {
        assert!(region.native_x87_group32(ordinal as u32).is_none());
    }
    assert!(region.wasm_bytes(&memory).unwrap().len() <= budget);
}

#[test]
fn native_steps_keep_literal_operand_segments_widths_and_terminal_store_metadata() {
    let mut memory = memory();
    memory
        .write(
            GuestAddress(0x1000),
            &[
                0x64, 0xd9, 0x05, 0x00, 0x20, 0x00, 0x00, // fld dword fs:[0x2000]
                0xd8, 0xc0, // fadd st0, st0
                0xd9, 0xc9, // fxch st1
                0xd9, 0x1d, 0x04, 0x20, 0x00, 0x00, // fstp dword [0x2004]
            ],
        )
        .unwrap();
    let region = CompiledRegion::new(
        prepared(&memory, 0x1000, 4),
        Vec::new(),
        super::super::CompilationTier32::Baseline,
        resident(),
    )
    .unwrap();
    let group = region.native_x87_group(0).unwrap();
    let steps = group.steps();
    assert_eq!(steps.len(), 4);
    assert_eq!(
        (steps[0].pc, steps[0].opcode, steps[0].instruction_length),
        (0x1000, 0x105, 7)
    );
    assert_eq!(steps[0].segment, SegmentRegister::Fs);
    assert_eq!((steps[0].read_size, steps[0].write_size), (4, 0));
    assert_eq!((steps[1].read_size, steps[1].write_size), (0, 0));
    assert_eq!(steps[3].pc, 0x100b);
    assert_eq!(steps[3].opcode, 0x11d);
    assert_eq!(steps[3].segment, SegmentRegister::Ds);
    assert_eq!((steps[3].read_size, steps[3].write_size), (0, 4));
    assert!(steps.iter().all(|step| step.waiting));
}
