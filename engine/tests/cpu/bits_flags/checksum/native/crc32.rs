use ring3_engine::{
    cpu::{
        UnsupportedFeature,
        x86::{
            Register32, SegmentRegister,
            bits::{Crc32Source, Crc32Width},
            decode::{DecodeError, decode_one},
            ir::{AddressSize, ByteRegister, EffectiveAddress, Operation},
        },
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

#[derive(Clone, Copy, Debug)]
enum Width {
    Byte,
    Word,
    Dword,
}

const FORMS: [(Width, &[u8]); 4] = [
    (Width::Byte, &[0xf2]),
    (Width::Word, &[0x66, 0xf2]),
    (Width::Word, &[0xf2, 0x66]),
    (Width::Dword, &[0xf2]),
];

const REGISTERS: [Register32; 8] = [
    Register32::Eax,
    Register32::Ecx,
    Register32::Edx,
    Register32::Ebx,
    Register32::Esp,
    Register32::Ebp,
    Register32::Esi,
    Register32::Edi,
];
const BYTE_REGISTERS: [ByteRegister; 8] = [
    ByteRegister::Al,
    ByteRegister::Cl,
    ByteRegister::Dl,
    ByteRegister::Bl,
    ByteRegister::Ah,
    ByteRegister::Ch,
    ByteRegister::Dh,
    ByteRegister::Bh,
];

fn crc_width(width: Width) -> Crc32Width {
    match width {
        Width::Byte => Crc32Width::Byte,
        Width::Word => Crc32Width::Word,
        Width::Dword => Crc32Width::Dword,
    }
}

fn expected_register(width: Width, destination: u8, source: u8) -> Operation {
    Operation::Crc32 {
        destination: REGISTERS[usize::from(destination)],
        source: match width {
            Width::Byte => Crc32Source::ByteRegister(BYTE_REGISTERS[usize::from(source)]),
            Width::Word => Crc32Source::WordRegister(REGISTERS[usize::from(source)]),
            Width::Dword => Crc32Source::DwordRegister(REGISTERS[usize::from(source)]),
        },
    }
}

fn expected_memory(width: Width, destination: u8, prefix: Option<u8>) -> Operation {
    let address = if prefix == Some(0x67) {
        EffectiveAddress {
            base: Some(Register32::Ebp),
            index: Some(Register32::Edi),
            address_size: AddressSize::Bits16,
            ..Default::default()
        }
    } else {
        EffectiveAddress {
            base: Some(Register32::Ebx),
            segment: match prefix {
                Some(0x64) => Some(SegmentRegister::Fs),
                Some(0x65) => Some(SegmentRegister::Gs),
                _ => None,
            },
            ..Default::default()
        }
    };
    Operation::Crc32 {
        destination: REGISTERS[usize::from(destination)],
        source: Crc32Source::Memory {
            width: crc_width(width),
            address,
        },
    }
}

fn encoded(width: Width, prefixes: &[u8], destination: u8, source: u8, memory: bool) -> Vec<u8> {
    let mut bytes = prefixes.to_vec();
    bytes.extend([
        0x0f,
        0x38,
        if matches!(width, Width::Byte) {
            0xf0
        } else {
            0xf1
        },
        (if memory { 0 } else { 0xc0 }) | (destination << 3) | source,
    ]);
    bytes
}

fn decode(bytes: &[u8]) -> Result<Operation, DecodeError> {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    memory
        .protect(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::EXECUTE,
        )
        .unwrap();
    let instruction = decode_one(&memory, GuestAddress(0x1000))?;
    assert_eq!(usize::from(instruction.length()), bytes.len());
    assert_eq!(
        instruction.next_pc(),
        GuestAddress(0x1000 + bytes.len() as u32)
    );
    Ok(*instruction.operation())
}

#[test]
fn crc32_register_forms_cover_all_destinations_sources_and_word_prefix_orders() {
    let mut cases = 0;
    for (width, prefixes) in FORMS {
        for destination in 0..8 {
            for source in 0..8 {
                let bytes = encoded(width, prefixes, destination, source, false);
                assert_eq!(
                    decode(&bytes),
                    Ok(expected_register(width, destination, source)),
                    "{width:?}: {bytes:02x?}"
                );
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 256);
}

#[test]
fn crc32_memory_forms_decode_without_reading_each_width_or_segmented_operand() {
    let mut cases = 0;
    for (width, prefixes) in FORMS {
        for destination in 0..8 {
            for extra_prefix in [None, Some(0x64), Some(0x65), Some(0x67)] {
                let mut bytes = extra_prefix.into_iter().collect::<Vec<_>>();
                // rm=3 is [ebx] in address32 and [bp+di] in address16.
                bytes.extend(encoded(width, prefixes, destination, 3, true));
                assert_eq!(
                    decode(&bytes),
                    Ok(expected_memory(width, destination, extra_prefix)),
                    "{width:?}: {bytes:02x?}"
                );
                cases += 1;
            }
        }
    }
    assert_eq!(cases, 128);
}

#[test]
fn crc32_rejects_lock_repeated_group_prefixes_and_non_crc_register_encodings() {
    for (width, prefixes) in FORMS {
        for memory in [false, true] {
            let bytes = encoded(width, prefixes, 0, 3, memory);
            for (prefix, expected) in [
                (0xf0, DecodeError::InvalidEncoding),
                (0xf2, DecodeError::Unsupported(UnsupportedFeature::Opcode)),
                (0xf3, DecodeError::Unsupported(UnsupportedFeature::Opcode)),
            ] {
                let mut malformed = vec![prefix];
                malformed.extend_from_slice(&bytes);
                assert_eq!(
                    decode(&malformed),
                    Err(expected),
                    "{width:?}: {malformed:02x?}"
                );
            }
        }
    }
    for opcode in [0xf0, 0xf1] {
        for prefixes in [&[][..], &[0xf3][..]] {
            let mut malformed = prefixes.to_vec();
            malformed.extend([0x0f, 0x38, opcode, 0xc3]);
            assert_eq!(
                decode(&malformed),
                Err(DecodeError::InvalidEncoding),
                "{malformed:02x?}"
            );
        }
    }
}
