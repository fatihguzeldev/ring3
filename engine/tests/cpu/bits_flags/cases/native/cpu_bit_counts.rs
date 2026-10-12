use ring3_engine::{
    cpu::x86::{
        Register32,
        bits::{BitCountKind, ScalarWidth},
        decode::{DecodeError, decode_one},
        ir::{EffectiveAddress, Operation, Value32},
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

fn decode(bytes: &[u8]) -> Result<Operation, DecodeError> {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    let instruction = decode_one(&memory, GuestAddress(0x1000))?;
    assert_eq!(usize::from(instruction.length()), bytes.len());
    Ok(*instruction.operation())
}

#[test]
fn count_families_cover_both_widths_registers_memory_and_prefix_order() {
    for (opcode, kind) in [
        (0xbc, BitCountKind::TrailingZero),
        (0xbd, BitCountKind::LeadingZero),
        (0xb8, BitCountKind::Population),
    ] {
        for (prefixes, width) in [
            (&[0xf3][..], ScalarWidth::Dword),
            (&[0x66, 0xf3][..], ScalarWidth::Word),
            (&[0xf3, 0x66][..], ScalarWidth::Word),
        ] {
            for (modrm, source) in [
                (0xc2, Value32::Register(Register32::Edx)),
                (
                    0x03,
                    Value32::Memory(EffectiveAddress {
                        base: Some(Register32::Ebx),
                        ..Default::default()
                    }),
                ),
            ] {
                let mut bytes = prefixes.to_vec();
                bytes.extend([0x0f, opcode, modrm]);
                let expected = if kind == BitCountKind::TrailingZero && width == ScalarWidth::Dword
                {
                    match source {
                        Value32::Register(source) => Operation::TrailingZeroCount32 {
                            destination: Register32::Eax,
                            source,
                        },
                        Value32::Memory(address) => Operation::ReadTrailingZeroCount32 {
                            destination: Register32::Eax,
                            address,
                        },
                        Value32::Immediate(_) => unreachable!(),
                    }
                } else {
                    Operation::BitCount {
                        kind,
                        width,
                        destination: Register32::Eax,
                        source,
                    }
                };
                assert_eq!(decode(&bytes).unwrap(), expected);
            }
        }
    }
}

#[test]
fn count_lock_is_invalid() {
    for opcode in [0xbc, 0xbd, 0xb8] {
        assert_eq!(
            decode(&[0xf0, 0xf3, 0x0f, opcode, 0x03]),
            Err(DecodeError::InvalidEncoding)
        );
    }
}
