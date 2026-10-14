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

fn rows() -> [(Code, &'static [u8]); 6] {
    [
        (Code::Xrstors_mem, &[0x0f, 0xc7, 0x18]),
        (Code::Xsaves_mem, &[0x0f, 0xc7, 0x28]),
        (
            Code::VEX_Vaesenc_ymm_ymm_ymmm256,
            &[0xc4, 0xe2, 0x7d, 0xdc, 0xc1],
        ),
        (
            Code::VEX_Vaesenclast_ymm_ymm_ymmm256,
            &[0xc4, 0xe2, 0x7d, 0xdd, 0xc1],
        ),
        (
            Code::VEX_Vaesdec_ymm_ymm_ymmm256,
            &[0xc4, 0xe2, 0x7d, 0xde, 0xc1],
        ),
        (
            Code::VEX_Vaesdeclast_ymm_ymm_ymmm256,
            &[0xc4, 0xe2, 0x7d, 0xdf, 0xc1],
        ),
    ]
}
fn executable(bytes: &[u8], pc: u32) -> AddressSpace {
    let mut memory = AddressSpace::new(2).unwrap();
    let page = PageRange::new(GuestAddress(pc & !0xfff), 1).unwrap();
    memory.map_zeroed(page, Permissions::ALL).unwrap();
    memory.write(GuestAddress(pc), bytes).unwrap();
    memory.protect(page, Permissions::EXECUTE).unwrap();
    memory
}
fn assert_ud(bytes: &[u8], code: Code) {
    let raw = Decoder::new(32, bytes, DecoderOptions::NONE).decode();
    assert_eq!(raw.code(), code);
    assert_eq!(raw.len(), bytes.len());
    let memory = executable(bytes, 0x1000);
    assert!(matches!(
        decode_one(&memory, GuestAddress(0x1000)),
        Err(DecodeError::Unsupported(_))
    ));
    let decoded = decode_for_execution(&memory, GuestAddress(0x1000)).unwrap();
    assert_eq!(
        decoded.operation(),
        &Operation::Control(ControlOperation::InvalidOpcode)
    );
    assert_eq!(decoded.segment_accesses(), 0);
    assert_eq!(usize::from(decoded.length()), bytes.len());
    assert!(memory.is_code_current(decoded.code_snapshot()));
}
#[test]
fn supervisor_xsave_and_wide_aes_absence_fault_before_operand_or_privilege_execution() {
    for (function, subfunction, register, bit) in [(13, 1, 0, 3), (7, 0, 2, 9)] {
        let leaf = CPU_ID_LEAVES
            .iter()
            .find(|leaf| leaf.function == function && leaf.subfunction == Some(subfunction))
            .unwrap();
        assert_eq!(leaf.registers[register] & (1 << bit), 0);
    }
    for (code, bytes) in rows() {
        assert_ud(bytes, code);
        let mut overridden = vec![0x64, 0x67];
        overridden.extend(bytes);
        assert_ud(&overridden, code);
        if bytes.len() == 5 {
            let mut memory_form = bytes.to_vec();
            memory_form[4] = 1;
            assert_ud(&memory_form, code);
        }
    }
}
#[test]
fn final_execute_byte_is_required_before_supervisor_xsave_or_wide_aes_classification() {
    for (code, bytes) in rows() {
        let pc = 0x2000 - bytes.len() as u32 + 1;
        let mut memory = executable(&bytes[..bytes.len() - 1], pc);
        let missing = decode_one(&memory, GuestAddress(pc)).unwrap_err();
        assert!(
            matches!(missing, DecodeError::Paging { error: crate::cpu::x86::CpuPagingError32::PageFault(fault), attempted_length: length, .. } if fault.linear() == GuestAddress(0x2000) && usize::from(length) == bytes.len())
        );
        assert_eq!(
            decode_for_execution(&memory, GuestAddress(pc)).unwrap_err(),
            missing
        );
        let page = PageRange::new(GuestAddress(0x2000), 1).unwrap();
        memory.map_zeroed(page, Permissions::ALL).unwrap();
        memory
            .write(GuestAddress(0x2000), &bytes[bytes.len() - 1..])
            .unwrap();
        memory.protect(page, Permissions::READ).unwrap();
        let denied = decode_one(&memory, GuestAddress(pc)).unwrap_err();
        assert!(
            matches!(denied, DecodeError::Paging { error: crate::cpu::x86::CpuPagingError32::PageFault(fault), .. } if fault.linear() == GuestAddress(0x2000))
        );
        assert_eq!(
            decode_for_execution(&memory, GuestAddress(pc)).unwrap_err(),
            denied
        );
        memory.protect(page, Permissions::EXECUTE).unwrap();
        let decoded = decode_for_execution(&memory, GuestAddress(pc)).unwrap();
        assert_eq!(
            decoded.operation(),
            &Operation::Control(ControlOperation::InvalidOpcode),
            "{code:?}"
        );
    }
}
#[test]
fn vex128_aes_selected_absence_and_neutral_prefix_refusals_remain_distinct() {
    for opcode in [0xdc, 0xdd, 0xde, 0xdf] {
        let memory = executable(&[0xc4, 0xe2, 0x79, opcode, 0xc1], 0x1000);
        let strict = decode_one(&memory, GuestAddress(0x1000)).unwrap();
        assert!(matches!(strict.operation(), Operation::Avx(_)));
        assert_eq!(
            decode_for_execution(&memory, GuestAddress(0x1000))
                .unwrap()
                .operation(),
            strict.operation()
        );
    }
    for bytes in [
        &[0xf2, 0xf2, 0x0f, 0x01, 0xe8][..],
        &[0xc4, 0xe3, 0x7d, 0x44, 0xc1, 0],
    ] {
        let memory = executable(bytes, 0x1000);
        let strict = decode_one(&memory, GuestAddress(0x1000)).unwrap_err();
        assert!(matches!(strict, DecodeError::Unsupported(_)));
        if bytes == [0xc4, 0xe3, 0x7d, 0x44, 0xc1, 0] {
            assert_eq!(
                decode_for_execution(&memory, GuestAddress(0x1000))
                    .unwrap()
                    .operation(),
                &Operation::Control(ControlOperation::InvalidOpcode)
            );
        } else {
            assert_eq!(
                decode_for_execution(&memory, GuestAddress(0x1000)).unwrap_err(),
                strict
            );
        }
    }
}
