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
    let page = PageRange::new(GuestAddress(0x1000), 1).unwrap();
    memory.map_zeroed(page, Permissions::ALL).unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    memory.protect(page, Permissions::EXECUTE).unwrap();
    memory
}

fn rows() -> Vec<Vec<u8>> {
    let mut rows = vec![vec![0xfa], vec![0xfb]];
    for opcode in [0x21, 0x23] {
        for debug in 0..8 {
            // mov dr ignores the mod bits and always selects a general-purpose register.
            for mode in [0, 0x40, 0x80, 0xc0] {
                for register in 0..8 {
                    rows.push(vec![0x0f, opcode, mode | debug << 3 | register]);
                }
            }
        }
    }
    rows
}

fn assert_fault(memory: &AddressSpace, bytes: &[u8], fault: ControlOperation) {
    let decoded = decode_for_execution(memory, GuestAddress(0x1000)).unwrap();
    assert_eq!(
        *decoded.operation(),
        Operation::Control(fault),
        "{bytes:02x?}"
    );
    assert_eq!(usize::from(decoded.length()), bytes.len());
    assert_eq!(decoded.segment_accesses(), 0);
    assert!(memory.is_code_current(decoded.code_snapshot()));
}

#[test]
fn fixed_user_environment_faults_cli_sti_and_every_movdr_register_encoding() {
    for bytes in rows() {
        let memory = executable(&bytes);
        assert!(matches!(
            decode_one(&memory, GuestAddress(0x1000)),
            Err(DecodeError::Unsupported(_))
        ));
        assert_fault(&memory, &bytes, ControlOperation::GeneralProtectionZero);
    }
    for row in [
        &[0xfa][..],
        &[0xfb],
        &[0x0f, 0x21, 0xe7],
        &[0x0f, 0x23, 0x2e],
    ] {
        for prefix in [0x66, 0x67, 0x64, 0x36] {
            let mut bytes = vec![prefix];
            bytes.extend(row);
            let memory = executable(&bytes);
            assert_fault(&memory, &bytes, ControlOperation::GeneralProtectionZero);
        }
    }
}

#[test]
fn invalid_lock_precedes_fixed_environment_privilege_faults() {
    for row in rows() {
        let mut bytes = vec![0xf0];
        bytes.extend(row);
        let memory = executable(&bytes);
        assert_eq!(
            decode_one(&memory, GuestAddress(0x1000)).unwrap_err(),
            DecodeError::InvalidEncoding
        );
        assert_fault(&memory, &bytes, ControlOperation::InvalidOpcode);
    }
}

#[test]
fn environment_faults_do_not_override_reserved_repeat_or_duplicate_profile_refusals() {
    for row in [
        &[0xfa][..],
        &[0xfb],
        &[0x0f, 0x21, 0xc0],
        &[0x0f, 0x23, 0xe8],
    ] {
        for prefix in [&[0xf2][..], &[0xf3], &[0x66, 0x66]] {
            let mut bytes = prefix.to_vec();
            bytes.extend(row);
            let memory = executable(&bytes);
            let strict = decode_one(&memory, GuestAddress(0x1000)).unwrap_err();
            assert!(matches!(strict, DecodeError::Unsupported(_)));
            assert_eq!(
                decode_for_execution(&memory, GuestAddress(0x1000)).unwrap_err(),
                strict,
                "{bytes:02x?}"
            );
        }
    }
}
