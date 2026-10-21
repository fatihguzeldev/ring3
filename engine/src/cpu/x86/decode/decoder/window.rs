use super::*;
use crate::{
    cpu::x86::{ControlOperation, SegmentCache, SegmentContext},
    memory::{PageRange, Permissions},
};

fn memory() -> AddressSpace {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory
}

#[test]
fn ordinary_instruction_ignores_trailing_invalid_encodings_in_both_code_sizes() {
    let mut memory = memory();
    let pc = GuestAddress(0x1000);
    for default32 in [false, true] {
        let mut segments = SegmentContext::default();
        segments.caches[1] = SegmentCache::from_bits(
            (segments.caches[1].bits() & !(1 << 25)) | (u32::from(default32) << 25),
        )
        .unwrap();
        let cs = CodeFetchContext::from_segments(&segments).unwrap();
        for bytes in [
            &[0x90][..],
            &[0xb8, 0x78, 0x56, 0x34, 0x12],
            &[0x64, 0x8b, 0x03],
            &[0xd9, 0xe8],
        ] {
            memory.write(pc, &[0xf0; 15]).unwrap();
            memory.write(pc, bytes).unwrap();
            let mut window = [0; 15];
            assert!(
                decode_window(&memory, pc, Some(cs), &mut window, cs.decoder_bitness()).is_some()
            );
            for policy in [DecodePolicy::Strict, DecodePolicy::Execution] {
                let BoundDecode::Instruction(instruction) =
                    decode_bound(&memory, pc, cs, policy).unwrap()
                else {
                    panic!("mapped instruction expected");
                };
                let length = if bytes[0] == 0xb8 && !default32 {
                    3
                } else {
                    bytes.len() as u8
                };
                assert_eq!(instruction.length(), length, "{bytes:02x?}");
                assert_eq!(instruction.pc(), pc);
                assert_eq!(
                    instruction.next_pc(),
                    GuestAddress(pc.0 + u32::from(length))
                );
                assert!(memory.is_code_current(instruction.code_snapshot()));
            }
        }
    }
}

#[test]
fn code_limit_accepts_short_instruction_and_preserves_first_denied_byte() {
    let mut memory = memory();
    let pc = GuestAddress(0x1000);
    let mut segments = SegmentContext::default();
    segments.caches[1] = SegmentCache::from_bits(0xf2b0_1000).unwrap();
    let cs = CodeFetchContext::from_segments(&segments).unwrap();
    memory.write(pc, &[0x90]).unwrap();
    assert!(decode_window(&memory, pc, Some(cs), &mut [0; 15], 32).is_none());
    for policy in [DecodePolicy::Strict, DecodePolicy::Execution] {
        let BoundDecode::Instruction(instruction) = decode_bound(&memory, pc, cs, policy).unwrap()
        else {
            panic!("one-byte instruction fits the code limit");
        };
        assert_eq!(instruction.length(), 1);
        assert_eq!(instruction.operation(), &Operation::Nop);
    }
    memory.write(pc, &[0xb8, 0x12, 0x34, 0x56, 0x78]).unwrap();
    assert!(matches!(
        decode_bound(&memory, pc, cs, DecodePolicy::Strict),
        Err(DecodeError::CodeSegment { pc: fault_pc, attempted_length: 2 }) if fault_pc == pc
    ));
    let BoundDecode::CodeFault(fault) =
        decode_bound(&memory, pc, cs, DecodePolicy::Execution).unwrap()
    else {
        panic!("first operand byte exceeds the code limit");
    };
    assert_eq!(fault.kind, CapturedFetchFaultKind32::Segment);
    assert_eq!(fault.attempted_length, 2);
    assert_eq!(fault.consumed_prefix_length, 1);
    assert!(memory.is_code_current(fault.prefix.as_ref().unwrap()));
}

#[test]
fn mapped_invalid_encoding_preserves_the_shortest_proven_fault_length() {
    let mut memory = memory();
    let pc = GuestAddress(0x1000);
    for (bytes, length) in [
        (&[0xf0, 0x90][..], 2),
        (&[0xf0, 0xf4][..], 2),
        (&[0x0f, 0x0a, 0][..], 3),
    ] {
        memory.write(pc, &[0x66; 15]).unwrap();
        memory.write(pc, bytes).unwrap();
        assert!(decode_window(&memory, pc, None, &mut [0; 15], 32).is_none());
        assert_eq!(
            decode_one(&memory, pc).unwrap_err(),
            DecodeError::InvalidEncoding
        );
        let instruction = decode_for_execution(&memory, pc).unwrap();
        assert_eq!(instruction.length(), length);
        assert_eq!(
            instruction.operation(),
            &Operation::Control(ControlOperation::InvalidOpcode)
        );
        assert!(memory.is_code_current(instruction.code_snapshot()));
    }
}

#[test]
fn window_probe_declines_linear_page_and_logical_address_wrap_boundaries() {
    let mut memory = memory();
    let page_end = GuestAddress(0x1fff);
    memory.write(page_end, &[0x90]).unwrap();
    assert!(decode_window(&memory, page_end, None, &mut [0; 15], 32).is_none());
    assert_eq!(decode_one(&memory, page_end).unwrap().length(), 1);

    memory.write(GuestAddress(0x1000), &[0x90]).unwrap();
    let mut segments = SegmentContext::default();
    segments.state.bases[1] = 0x1001;
    let cs = CodeFetchContext::from_segments(&segments).unwrap();
    let pc = GuestAddress(u32::MAX);
    assert_eq!(cs.linear_address(pc), GuestAddress(0x1000));
    assert!(decode_window(&memory, pc, Some(cs), &mut [0; 15], 32).is_none());
    let BoundDecode::Instruction(instruction) =
        decode_bound(&memory, pc, cs, DecodePolicy::Execution).unwrap()
    else {
        panic!("one-byte logical boundary instruction expected");
    };
    assert_eq!(instruction.length(), 1);
    assert_eq!(instruction.next_pc(), GuestAddress(0));
}

#[test]
fn reused_scratch_observes_code_replacement_and_execute_mapping_repair() {
    let mut memory = memory();
    let mut scratch = DecodeScratch::default();
    let pc = GuestAddress(0x1000);
    let cs = CodeFetchContext::from_segments(&SegmentContext::default()).unwrap();
    let page = PageRange::new(pc, 1).unwrap();
    memory.write(pc, &[0xb8, 1, 0, 0, 0]).unwrap();
    let BoundDecode::Instruction(first) = scratch
        .decode(
            &memory,
            pc,
            DecodePolicy::Execution,
            Some(cs),
            DecodeAuthority::default(),
        )
        .unwrap()
    else {
        panic!("first immediate move expected");
    };
    memory.write(pc, &[0xb8, 2, 0, 0, 0]).unwrap();
    let BoundDecode::Instruction(replaced) = scratch
        .decode(
            &memory,
            pc,
            DecodePolicy::Execution,
            Some(cs),
            DecodeAuthority::default(),
        )
        .unwrap()
    else {
        panic!("replacement immediate move expected");
    };
    assert_ne!(first.operation(), replaced.operation());
    memory.protect(page, Permissions::READ).unwrap();
    let BoundDecode::CodeFault(fault) = scratch
        .decode(
            &memory,
            pc,
            DecodePolicy::Execution,
            Some(cs),
            DecodeAuthority::default(),
        )
        .unwrap()
    else {
        panic!("execute permission denial expected");
    };
    assert_eq!(fault.kind, CapturedFetchFaultKind32::Paging);
    assert_eq!(fault.attempted_length, 1);
    assert!(fault.prefix.is_none());
    memory.protect(page, Permissions::ALL).unwrap();
    assert!(!fault.remains_denied(&memory, cs));
    memory.unmap(page).unwrap();
    memory.map_zeroed(page, Permissions::ALL).unwrap();
    memory.write(pc, &[0x90]).unwrap();
    let BoundDecode::Instruction(remapped) = scratch
        .decode(
            &memory,
            pc,
            DecodePolicy::Execution,
            Some(cs),
            DecodeAuthority::default(),
        )
        .unwrap()
    else {
        panic!("remapped instruction expected");
    };
    assert_eq!(remapped.operation(), &Operation::Nop);
    assert_eq!(remapped.length(), 1);
}
