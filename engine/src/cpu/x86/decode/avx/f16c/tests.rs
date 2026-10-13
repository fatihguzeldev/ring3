use super::*;
use iced_x86::{Decoder, DecoderError, DecoderOptions};

#[test]
fn primary_reserved_w_and_vvvv_are_rejected_independently_of_iced() {
    for (map, opcode, immediate) in [(0xe2, 0x13, None), (0xe3, 0x1d, Some(255))] {
        for control in [0x79, 0x7d] {
            let mut bytes = vec![0xc4, map, control, opcode, 0xcb];
            bytes.extend(immediate);
            let mut decoder = Decoder::new(32, &bytes, DecoderOptions::NONE);
            let instruction = decoder.decode();
            assert_eq!(decoder.last_error(), DecoderError::None);
            assert!(
                lower(&instruction, Encoding::parse(&bytes).unwrap())
                    .unwrap()
                    .is_ok()
            );
            for invalid_control in [control | 0x80, control & !8] {
                bytes[2] = invalid_control;
                assert_eq!(
                    lower(&instruction, Encoding::parse(&bytes).unwrap()),
                    Some(Err(DecodeError::InvalidEncoding))
                );
            }
        }
    }
}

#[test]
fn canonical_guest_gate_matches_cpuid_exposure_without_changing_private_lowering() {
    use crate::{
        cpu::x86::{capabilities, decode::decode_one, ir::Operation},
        memory::{AddressSpace, GuestAddress, PageRange, Permissions},
    };
    let bytes = [0xc4, 0xe2, 0x79, 0x13, 0xcb];
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), &bytes).unwrap();
    let result = decode_one(&memory, GuestAddress(0x1000));
    if capabilities::F16C_ENABLED {
        assert!(matches!(result.unwrap().operation(), Operation::Avx(_)));
    } else {
        assert_eq!(
            result.unwrap_err(),
            DecodeError::Unsupported(UnsupportedFeature::Simd)
        );
    }
    let features = capabilities::CPU_ID_LEAVES
        .iter()
        .find(|leaf| leaf.function == 1)
        .unwrap()
        .registers[2];
    assert_eq!(features & (1 << 29) != 0, capabilities::F16C_ENABLED);
}
