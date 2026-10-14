use crate::{
    cpu::x86::{
        ControlOperation,
        decode::{DecodeError, decode_for_execution, decode_one},
        ir::Operation,
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

fn executable(bytes: &[u8]) -> AddressSpace {
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
    memory
}

fn privileged_rows() -> Vec<Vec<u8>> {
    let mut rows = vec![
        vec![0x0f, 0x06],
        vec![0x0f, 0x01, 0x38],
        vec![0x0f, 0x01, 0x10],
        vec![0x0f, 0x01, 0x18],
        vec![0x0f, 0x00, 0xd0],
        vec![0x0f, 0x00, 0x10],
        vec![0x0f, 0x00, 0xd8],
        vec![0x0f, 0x00, 0x18],
        vec![0x0f, 0x01, 0xf0],
        vec![0x0f, 0x01, 0x30],
        vec![0x0f, 0x08],
        vec![0x0f, 0x09],
        vec![0x0f, 0x32],
        vec![0x0f, 0x30],
        vec![0x0f, 0x33],
        vec![0xe4, 0x80],
        vec![0xe6, 0x80],
    ];
    for opcode in [0x20, 0x22] {
        for control in [0, 2, 3, 4] {
            // the mod bits are ignored by mov cr; all encodings select a register.
            for mode in [0, 0x40, 0x80, 0xc0] {
                for register in 0..8 {
                    rows.push(vec![0x0f, opcode, mode | control << 3 | register]);
                }
            }
        }
    }
    rows
}

#[test]
fn cpl3_privileged_whitelist_faults_without_accessing_an_operand() {
    for row in privileged_rows() {
        for prefix in [&[][..], &[0x66], &[0x67], &[0x64], &[0x36]] {
            let mut bytes = prefix.to_vec();
            bytes.extend(&row);
            let memory = executable(&bytes);
            assert!(matches!(
                decode_one(&memory, GuestAddress(0x1000)),
                Err(DecodeError::Unsupported(_))
            ));
            let decoded = decode_for_execution(&memory, GuestAddress(0x1000)).unwrap();
            assert_eq!(
                *decoded.operation(),
                Operation::Control(ControlOperation::GeneralProtectionZero),
                "{bytes:02x?}"
            );
            assert_eq!(usize::from(decoded.length()), bytes.len());
            assert_eq!(decoded.segment_accesses(), 0);
            assert!(memory.is_code_current(decoded.code_snapshot()));
        }
    }
}

#[test]
fn privileged_lock_invalidity_precedes_cpl3_general_protection() {
    for row in privileged_rows() {
        let mut bytes = vec![0xf0];
        bytes.extend(row);
        let memory = executable(&bytes);
        assert_eq!(
            decode_one(&memory, GuestAddress(0x1000)).unwrap_err(),
            DecodeError::InvalidEncoding
        );
        let decoded = decode_for_execution(&memory, GuestAddress(0x1000)).unwrap();
        assert_eq!(
            *decoded.operation(),
            Operation::Control(ControlOperation::InvalidOpcode),
            "{bytes:02x?}"
        );
        assert_eq!(decoded.segment_accesses(), 0);
    }
}

#[test]
fn privileged_prefix_refusals_distinguish_the_disabled_writeback_alias() {
    for row in privileged_rows() {
        for prefix in [&[0xf2][..], &[0xf3], &[0x66, 0x66]] {
            let mut bytes = prefix.to_vec();
            bytes.extend(&row);
            let memory = executable(&bytes);
            let strict = decode_one(&memory, GuestAddress(0x1000)).unwrap_err();
            assert!(matches!(strict, DecodeError::Unsupported(_)));
            if bytes == [0xf3, 0x0f, 0x09] {
                let decoded = decode_for_execution(&memory, GuestAddress(0x1000)).unwrap();
                assert_eq!(
                    decoded.operation(),
                    &Operation::Control(ControlOperation::GeneralProtectionZero)
                );
                assert_eq!(decoded.segment_accesses(), 0);
            } else {
                assert_eq!(
                    decode_for_execution(&memory, GuestAddress(0x1000)).unwrap_err(),
                    strict,
                    "{bytes:02x?}"
                );
            }
        }
    }
}

#[test]
fn disabled_writeback_fault_and_software_interrupt_admission_remain_distinct() {
    let memory = executable(&[0xf3, 0x0f, 0x09]);
    assert!(matches!(
        decode_one(&memory, GuestAddress(0x1000)),
        Err(DecodeError::Unsupported(_))
    ));
    let decoded = decode_for_execution(&memory, GuestAddress(0x1000)).unwrap();
    assert_eq!(
        decoded.operation(),
        &Operation::Control(ControlOperation::GeneralProtectionZero)
    );
    assert_eq!(decoded.segment_accesses(), 0);

    let memory = executable(&[0xcd, 0x80]);
    for decoded in [
        decode_one(&memory, GuestAddress(0x1000)).unwrap(),
        decode_for_execution(&memory, GuestAddress(0x1000)).unwrap(),
    ] {
        assert_eq!(
            decoded.operation(),
            &Operation::SoftwareInterrupt { vector: 0x80 }
        );
        assert_eq!(decoded.length(), 2);
        assert_eq!(decoded.segment_accesses(), 0);
    }
}

#[test]
fn reserved_control_registers_remain_invalid_before_privilege_classification() {
    for opcode in [0x20, 0x22] {
        for control in [1, 5, 6, 7] {
            let bytes = [0x0f, opcode, 0xc0 | control << 3];
            let memory = executable(&bytes);
            assert_eq!(
                decode_one(&memory, GuestAddress(0x1000)).unwrap_err(),
                DecodeError::InvalidEncoding
            );
            let decoded = decode_for_execution(&memory, GuestAddress(0x1000)).unwrap();
            assert_eq!(
                *decoded.operation(),
                Operation::Control(ControlOperation::InvalidOpcode),
                "{bytes:02x?}"
            );
        }
    }
}
