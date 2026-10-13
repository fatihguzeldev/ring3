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
fn virtual_syscall_absence_not_a_guessed_efer_value_selects_ud_for_both_codes() {
    let leaf = CPU_ID_LEAVES
        .iter()
        .find(|leaf| leaf.function == 0x80000001)
        .unwrap();
    assert_eq!(leaf.registers[3] & (1 << 11), 0);
    for (bytes, code) in [
        (&[0x0f, 0x05][..], Code::Syscall),
        (&[0x0f, 0x07][..], Code::Sysretd),
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
