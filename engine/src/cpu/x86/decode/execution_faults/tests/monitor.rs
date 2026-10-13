use crate::{
    cpu::x86::{
        ControlOperation,
        decode::{DecodeError, decode_for_execution, decode_one},
        environment::USER_MODE_ENVIRONMENT,
        ir::Operation,
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};
use iced_x86::{Code, Decoder, DecoderOptions};

#[test]
fn exact_cpl3_monitor_and_mwait_forms_fault_without_any_operand_segment_access() {
    assert_eq!(USER_MODE_ENVIRONMENT.current_privilege(), 3);
    for (bytes, code) in [
        (&[0x0f, 0x01, 0xc8][..], Code::Monitord),
        (&[0x67, 0x0f, 0x01, 0xc8], Code::Monitorw),
        (&[0x0f, 0x01, 0xc9], Code::Mwait),
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
