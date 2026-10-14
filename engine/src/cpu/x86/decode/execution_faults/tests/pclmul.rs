use crate::{
    cpu::x86::{
        ControlOperation,
        capabilities::CPU_ID_LEAVES,
        decode::{DecodeError, decode_for_execution, decode_one},
        ir::Operation,
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};
use iced_x86::{Code, Decoder, DecoderOptions};
fn memory(bytes: &[u8]) -> AddressSpace {
    let mut m = AddressSpace::new(1).unwrap();
    m.map_zeroed(
        PageRange::new(GuestAddress(0x1000), 1).unwrap(),
        Permissions::ALL,
    )
    .unwrap();
    m.write(GuestAddress(0x1000), bytes).unwrap();
    m
}
#[test]
fn exact_wide_carryless_code_faults_with_vpclmul_absent_and_narrow_stays_available() {
    let leaf = CPU_ID_LEAVES
        .iter()
        .find(|leaf| leaf.function == 7 && leaf.subfunction == Some(0))
        .unwrap();
    assert_eq!(leaf.registers[2] & (1 << 10), 0);
    for bytes in [
        &[0xc4, 0xe3, 0x7d, 0x44, 0xc1, 0][..],
        &[0xc4, 0xe3, 0x7d, 0x44, 1, 0],
        &[0x64, 0x67, 0xc4, 0xe3, 0x7d, 0x44, 1, 0],
    ] {
        let raw = Decoder::new(32, bytes, DecoderOptions::NONE).decode();
        assert_eq!(raw.code(), Code::VEX_Vpclmulqdq_ymm_ymm_ymmm256_imm8);
        assert_eq!(raw.len(), bytes.len());
        let m = memory(bytes);
        assert!(matches!(
            decode_one(&m, GuestAddress(0x1000)),
            Err(DecodeError::Unsupported(_))
        ));
        let d = decode_for_execution(&m, GuestAddress(0x1000)).unwrap();
        assert_eq!(
            d.operation(),
            &Operation::Control(ControlOperation::InvalidOpcode)
        );
        assert_eq!(d.segment_accesses(), 0);
    }
    let m = memory(&[0xc4, 0xe3, 0x79, 0x44, 0xc1, 0]);
    let strict = decode_one(&m, GuestAddress(0x1000)).unwrap();
    assert!(matches!(strict.operation(), Operation::Avx(_)));
    assert_eq!(
        decode_for_execution(&m, GuestAddress(0x1000))
            .unwrap()
            .operation(),
        strict.operation()
    );
}
