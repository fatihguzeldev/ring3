use ring3_engine::{
    cpu::x86::{
        Register32,
        bits::ScalarWidth,
        decode::{DecodeError, decode_one},
        ir::{EffectiveAddress, Location32, Operation, Value32},
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

#[test]
fn movbe_load_and_store_keep_width_and_require_one_memory_operand() {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    let address = EffectiveAddress {
        base: Some(Register32::Ebx),
        ..Default::default()
    };
    for (prefix, width) in [
        (&[][..], ScalarWidth::Dword),
        (&[0x66][..], ScalarWidth::Word),
    ] {
        for opcode in [0xf0, 0xf1] {
            let mut bytes = prefix.to_vec();
            bytes.extend([0x0f, 0x38, opcode, 0x03]);
            memory.write(GuestAddress(0x1000), &bytes).unwrap();
            assert_eq!(
                *decode_one(&memory, GuestAddress(0x1000))
                    .unwrap()
                    .operation(),
                Operation::MoveByteSwap {
                    width,
                    destination: if opcode == 0xf0 {
                        Location32::Register(Register32::Eax)
                    } else {
                        Location32::Memory(address)
                    },
                    source: if opcode == 0xf0 {
                        Value32::Memory(address)
                    } else {
                        Value32::Register(Register32::Eax)
                    },
                }
            );
            *bytes.last_mut().unwrap() = 0xc3;
            memory.write(GuestAddress(0x1000), &bytes).unwrap();
            assert_eq!(
                decode_one(&memory, GuestAddress(0x1000)).err(),
                Some(DecodeError::InvalidEncoding)
            );
        }
    }
}

#[test]
fn movbe_does_not_accept_lock_or_repeat_prefixes() {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    for prefix in [0xf0, 0xf3] {
        memory
            .write(GuestAddress(0x1000), &[prefix, 0x0f, 0x38, 0xf0, 0x03])
            .unwrap();
        assert!(decode_one(&memory, GuestAddress(0x1000)).is_err());
    }
}
