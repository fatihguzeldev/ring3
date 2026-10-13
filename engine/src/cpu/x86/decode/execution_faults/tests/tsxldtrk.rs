use crate::{
    cpu::x86::{
        ControlOperation,
        capabilities::CPU_ID_LEAVES,
        decode::{DecodeError, decode_for_execution},
        ir::Operation,
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};
use iced_x86::{Code, Decoder, DecoderOptions};

#[test]
fn exact_mandatory_prefix_forms_fault_with_tsxldtrk_absent() {
    let leaf = CPU_ID_LEAVES
        .iter()
        .find(|leaf| leaf.function == 7 && leaf.subfunction == Some(0))
        .unwrap();
    assert_eq!(leaf.registers[3] & (1 << 16), 0);
    for (last, code) in [(0xe8, Code::Xsusldtrk), (0xe9, Code::Xresldtrk)] {
        let bytes = [0xf2, 0x0f, 0x01, last];
        let instruction = Decoder::new(32, &bytes, DecoderOptions::NONE).decode();
        assert_eq!(instruction.code(), code);
        assert!(
            !instruction.has_repne_prefix(),
            "mandatory F2 belongs to the exact opcode"
        );
        let mut memory = AddressSpace::new(1).unwrap();
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(0x3000), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
        memory.write(GuestAddress(0x3000), &bytes).unwrap();
        let decoded = decode_for_execution(&memory, GuestAddress(0x3000)).unwrap();
        assert_eq!(
            decoded.operation(),
            &Operation::Control(ControlOperation::InvalidOpcode)
        );
        assert_eq!(decoded.length(), 4);
        assert_eq!(decoded.segment_accesses(), 0);
    }
}

#[test]
fn incomplete_mandatory_form_faults_fetch_before_availability() {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x3000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory
        .write(GuestAddress(0x3ffd), &[0xf2, 0x0f, 0x01])
        .unwrap();
    assert!(
        matches!(decode_for_execution(&memory, GuestAddress(0x3ffd)),
        Err(DecodeError::MemoryFault { fault, .. }) if fault.address == GuestAddress(0x4000))
    );
}
