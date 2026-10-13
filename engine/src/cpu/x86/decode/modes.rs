use super::*;
use crate::{
    cpu::x86::{
        Register32, SegmentCache, SegmentContext,
        ir::{AddressSize, Location32, Operation, Value32, WordValue},
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

fn decoded(bytes: &[u8], default32: bool) -> DecodedInstruction {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    let mut context = SegmentContext::default();
    if !default32 {
        context.caches[1] = SegmentCache::from_bits(0xf1bf_ffff).unwrap();
    }
    let cs = CodeFetchContext::from_segments(&context).unwrap();
    let BoundDecode::Instruction(decoded) =
        decode_bound(&memory, GuestAddress(0x1000), cs, DecodePolicy::Strict).unwrap()
    else {
        panic!("valid full-limit source must decode material");
    };
    assert!(memory.is_code_current(decoded.code_snapshot()));
    decoded
}

#[test]
fn same_physical_bytes_have_distinct_retained_default_interpretations() {
    let word = decoded(&[0xb8, 0x34, 0x12, 0x90, 0x90], false);
    assert_eq!((word.length(), word.next_pc().0), (3, 0x1003));
    assert!(matches!(
        word.operation(),
        Operation::MoveWord {
            destination: Register32::Eax,
            source: WordValue::Immediate(0x1234)
        }
    ));
    let dword = decoded(&[0xb8, 0x34, 0x12, 0x90, 0x90], true);
    assert_eq!((dword.length(), dword.next_pc().0), (5, 0x1005));
    assert!(matches!(
        dword.operation(),
        Operation::Move {
            destination: Location32::Register(Register32::Eax),
            source: Value32::Immediate(0x90901234)
        }
    ));
}

#[test]
fn code_default_and_each_override_select_operand_and_address_independently() {
    for (bytes, word, address_size) in [
        (&[0x8b, 0x10][..], true, AddressSize::Bits16),
        (&[0x66, 0x8b, 0x10][..], false, AddressSize::Bits16),
        (&[0x67, 0x8b, 0x10][..], true, AddressSize::Bits32),
        (&[0x66, 0x67, 0x8b, 0x10][..], false, AddressSize::Bits32),
    ] {
        let instruction = decoded(bytes, false);
        let address = match instruction.operation() {
            Operation::LoadWord {
                destination: Register32::Edx,
                address,
            } => {
                assert!(word);
                *address
            }
            Operation::Move {
                destination: Location32::Register(Register32::Edx),
                source: Value32::Memory(address),
            } => {
                assert!(!word);
                *address
            }
            operation => panic!("unexpected default/override operation: {operation:?}"),
        };
        assert_eq!(address.address_size, address_size);
        assert_eq!(
            address.base,
            Some(if address_size == AddressSize::Bits16 {
                Register32::Ebx
            } else {
                Register32::Eax
            })
        );
        assert_eq!(
            address.index,
            if address_size == AddressSize::Bits16 {
                Some(Register32::Esi)
            } else {
                None
            }
        );
    }
    for (bytes, address_size) in [
        (&[0xa1, 0x20, 0][..], AddressSize::Bits16),
        (&[0x67, 0xa1, 0x20, 0, 0, 0][..], AddressSize::Bits32),
    ] {
        let instruction = decoded(bytes, false);
        let Operation::LoadWord { address, .. } = instruction.operation() else {
            panic!("moffs must load a word");
        };
        assert_eq!(
            (address.address_size, address.displacement),
            (address_size, 0x20)
        );
        assert_eq!((address.base, address.index), (None, None));
    }
    assert!(
        matches!(decoded(&[0xd7], false).operation(), Operation::TranslateByteAddress { address } if address.address_size == AddressSize::Bits16)
    );
    assert!(matches!(
        decoded(&[0x67, 0xd7], false).operation(),
        Operation::TranslateByte
    ));
}
