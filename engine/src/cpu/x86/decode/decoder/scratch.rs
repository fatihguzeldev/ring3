use super::*;
use crate::{
    cpu::x86::SegmentContext,
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

fn decoded(memory: &AddressSpace, scratch: &mut DecodeScratch) -> DecodedOperation {
    let cs = CodeFetchContext::from_segments(&SegmentContext::default()).unwrap();
    let BoundDecode::Instruction(instruction) = scratch
        .decode(
            memory,
            GuestAddress(0x1000),
            DecodePolicy::Execution,
            Some(cs),
            DecodeAuthority::default(),
        )
        .unwrap()
    else {
        panic!("complete mapped instruction");
    };
    instruction
}

#[test]
fn owned_scratch_preserves_literal_segment_masks_across_reuse() {
    let mut memory = memory();
    let mut scratch = DecodeScratch::default();
    for (bytes, expected) in [
        (&[0x8b, 0x03][..], 8),
        (&[0x64, 0x8b, 0x03][..], 16),
        (&[0x65, 0x8b, 0x03][..], 32),
        (&[0x8b, 0x45, 0][..], 4),
        (&[0x50][..], 4),
        (&[0xf3, 0xa4][..], 9),
        (&[0x8b, 0xc1][..], 0),
        (&[0x8d, 0x03][..], 0),
        (&[0x0f, 0x18, 0x03][..], 0),
        (&[0xcd, 0x80][..], 0),
        (&[0xd9, 0x03][..], 8),
        (&[0xd9, 0xe8][..], 0),
    ] {
        memory.write(GuestAddress(0x1000), bytes).unwrap();
        let actual = decoded(&memory, &mut scratch);
        assert_eq!(actual.segment_accesses(), expected, "{bytes:02x?}");
        let cs = CodeFetchContext::from_segments(&SegmentContext::default()).unwrap();
        let BoundDecode::Instruction(original) = decode_for_compilation(
            &memory,
            GuestAddress(0x1000),
            DecodePolicy::Execution,
            Some(cs),
            DecodeAuthority::default(),
        )
        .unwrap() else {
            panic!("complete mapped instruction");
        };
        assert_eq!(actual.operation(), original.operation());
        assert_eq!(actual.pc(), original.pc());
        assert_eq!(actual.next_pc(), original.next_pc());
        assert_eq!(actual.length(), original.length());
    }
}

#[test]
fn owned_scratch_retains_the_factory_storage_used_by_decode() {
    let mut memory = memory();
    let mut scratch = DecodeScratch::default();
    memory
        .write(GuestAddress(0x1000), &[0x64, 0x8b, 0x03])
        .unwrap();
    assert_eq!(decoded(&memory, &mut scratch).segment_accesses(), 16);
    let storage = scratch
        .info_storage
        .expect("decode uses owned factory storage");
    for bytes in [
        &[0x8b, 0x45, 0][..],
        &[0x8b, 0xc1][..],
        &[0xf3, 0xa4][..],
        &[0x8b, 0x03][..],
    ] {
        memory.write(GuestAddress(0x1000), bytes).unwrap();
        decoded(&memory, &mut scratch);
        assert_eq!(scratch.info_storage, Some(storage), "{bytes:02x?}");
    }
    let mut independent = DecodeScratch::default();
    decoded(&memory, &mut independent);
    let other = independent
        .info_storage
        .expect("independent decode owns storage");
    assert_ne!(other[0], storage[0]);
    assert_ne!(other[1], storage[1]);
}
