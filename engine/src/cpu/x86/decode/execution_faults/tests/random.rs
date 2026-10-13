use crate::{
    cpu::{
        UnsupportedFeature,
        x86::{
            ControlOperation,
            capabilities::CPU_ID_LEAVES,
            decode::{DecodeError, decode_for_execution, decode_one},
            ir::Operation,
        },
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};
use iced_x86::{Code, Decoder, DecoderOptions};

#[test]
fn advertised_absence_and_exact_register_forms_select_ud_without_operand_authority() {
    let basic = CPU_ID_LEAVES
        .iter()
        .find(|leaf| leaf.function == 1)
        .unwrap();
    let structured = CPU_ID_LEAVES
        .iter()
        .find(|leaf| leaf.function == 7 && leaf.subfunction == Some(0))
        .unwrap();
    assert_eq!(basic.registers[2] & (1 << 30), 0);
    assert_eq!(structured.registers[1] & (1 << 18), 0);
    for (bytes, code) in [
        (&[0x0f, 0xc7, 0xf0][..], Code::Rdrand_r32),
        (&[0x66, 0x0f, 0xc7, 0xf0], Code::Rdrand_r16),
        (&[0x0f, 0xc7, 0xf8], Code::Rdseed_r32),
        (&[0x66, 0x0f, 0xc7, 0xf8], Code::Rdseed_r16),
    ] {
        let pc = 0x2000 - bytes.len() as u32;
        let mut raw = Decoder::with_ip(32, bytes, u64::from(pc), DecoderOptions::NONE);
        assert_eq!(raw.decode().code(), code);
        let mut memory = AddressSpace::new(1).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x1000), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        memory.write(GuestAddress(pc), bytes).unwrap();
        assert_eq!(
            decode_one(&memory, GuestAddress(pc)).unwrap_err(),
            DecodeError::Unsupported(UnsupportedFeature::Opcode)
        );
        let decoded = decode_for_execution(&memory, GuestAddress(pc)).unwrap();
        assert_eq!(
            decoded.operation(),
            &Operation::Control(ControlOperation::InvalidOpcode)
        );
        assert_eq!(decoded.segment_accesses(), 0);
        assert_eq!(decoded.next_pc(), GuestAddress(0x2000));
        assert_eq!(decoded.length() as usize, bytes.len());
        assert!(memory.is_code_current(decoded.code_snapshot()));
    }
}
