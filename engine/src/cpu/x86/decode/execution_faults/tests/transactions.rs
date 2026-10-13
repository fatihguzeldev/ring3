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
#[test]
fn exact_rtm_and_xtest_absence_selects_ud_without_transaction_state() {
    let leaf = CPU_ID_LEAVES
        .iter()
        .find(|leaf| leaf.function == 7 && leaf.subfunction == Some(0))
        .unwrap();
    assert_eq!(leaf.registers[1] & ((1 << 4) | (1 << 11)), 0);
    for (bytes, code) in [
        (&[198, 248, 90][..], Code::Xabort_imm8),
        (&[102, 199, 248, 0, 0][..], Code::Xbegin_rel16),
        (&[199, 248, 0, 0, 0, 0][..], Code::Xbegin_rel32),
        (&[15, 1, 213][..], Code::Xend),
        (&[15, 1, 214][..], Code::Xtest),
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
        assert!(matches!(
            decode_one(&memory, GuestAddress(pc)),
            Err(DecodeError::Unsupported(_))
        ));
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
