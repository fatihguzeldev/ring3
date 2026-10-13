use super::*;
use crate::{
    cpu::x86::{SegmentCache, SegmentContext},
    memory::{PageRange, Permissions},
};

#[test]
fn translated_gate_wrap_captures_each_executable_linear_page() {
    for address in [u32::MAX, 0] {
        let mut memory = AddressSpace::new(3).unwrap();
        for page in [0, 0x3000, 0xfffff000] {
            memory
                .map_zeroed(
                    PageRange::new(GuestAddress(page), 1).unwrap(),
                    Permissions::ALL,
                )
                .unwrap();
        }
        memory.write(GuestAddress(u32::MAX), &[0x0f]).unwrap();
        memory.write(GuestAddress(0), &[0x0b]).unwrap();
        let mut context = SegmentContext::default();
        context.state.bases[1] = 0xffffe000;
        let cs = CodeFetchContext::from_segments(&context).unwrap();
        let gate = prepare(
            &memory,
            GateSpec {
                entry: GuestAddress(0x1fff),
                id: 77,
            },
            Some(cs),
        )
        .unwrap();
        memory.write(GuestAddress(0x3000), &[0x90]).unwrap();
        assert!(gate.is_current(&memory));
        memory
            .write(
                GuestAddress(address),
                &[if address == 0 { 0x0b } else { 0x0f }],
            )
            .unwrap();
        assert!(!gate.is_current(&memory));
    }
}

#[test]
fn logical_gate_limit_still_precedes_unmapped_wrapped_linear_tail() {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0xfffff000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(u32::MAX), &[0x0f]).unwrap();
    let mut context = SegmentContext::default();
    context.state.bases[1] = 0xffffe000;
    context.caches[1] = SegmentCache::from_bits(0xf2b01fff).unwrap();
    let cs = CodeFetchContext::from_segments(&context).unwrap();
    assert!(matches!(
        prepare(
            &memory,
            GateSpec {
                entry: GuestAddress(0x1fff),
                id: 77
            },
            Some(cs)
        ),
        Err(CompileError::Instruction {
            pc: GuestAddress(0x1fff),
            cause: InstructionError::Decode(DecodeError::CodeSegment {
                attempted_length: 2,
                ..
            }),
        })
    ));
}
