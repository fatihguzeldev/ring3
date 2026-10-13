use crate::{
    cpu::x86::{
        ControlOperation,
        decode::{DecodeError, decode_for_execution, decode_one},
        ir::{AddressSize, Operation},
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

fn forms() -> [&'static [u8]; 6] {
    [
        &[0x6c],
        &[0x66, 0x6d],
        &[0x6d],
        &[0x6e],
        &[0x66, 0x6f],
        &[0x6f],
    ]
}

#[test]
fn exact_port_string_codes_capture_only_optional_selected_repeat_count() {
    for form in forms() {
        for address16 in [false, true] {
            for repeat in [false, true] {
                let mut bytes = Vec::new();
                if address16 {
                    bytes.push(0x67);
                }
                if repeat {
                    bytes.push(0xf3);
                }
                bytes.extend(form);
                let memory = executable(&bytes);
                assert!(matches!(
                    decode_one(&memory, GuestAddress(0x1000)),
                    Err(DecodeError::Unsupported(_))
                ));
                let decoded = decode_for_execution(&memory, GuestAddress(0x1000)).unwrap();
                let operation = ControlOperation::DeniedPortString {
                    repeat_count: repeat.then_some(if address16 {
                        AddressSize::Bits16
                    } else {
                        AddressSize::Bits32
                    }),
                };
                assert_eq!(
                    *decoded.operation(),
                    Operation::Control(operation),
                    "{bytes:02x?}"
                );
                assert!(!operation.reads_memory());
                assert_eq!(usize::from(decoded.length()), bytes.len());
                assert_eq!(decoded.next_pc(), GuestAddress(0x1000 + bytes.len() as u32));
                assert_eq!(decoded.segment_accesses(), 0);
                assert!(memory.is_code_current(decoded.code_snapshot()));
            }
        }
    }
}

#[test]
fn nominal_segment_overrides_do_not_create_an_operand_access() {
    for opcode in [0x6c, 0x6e] {
        for prefix in [0x26, 0x2e, 0x36, 0x3e, 0x64, 0x65, 0x66] {
            let bytes = [prefix, 0xf3, opcode];
            let memory = executable(&bytes);
            let decoded = decode_for_execution(&memory, GuestAddress(0x1000)).unwrap();
            assert_eq!(
                *decoded.operation(),
                Operation::Control(ControlOperation::DeniedPortString {
                    repeat_count: Some(AddressSize::Bits32),
                })
            );
            assert_eq!(decoded.segment_accesses(), 0);
            assert_eq!(decoded.length(), 3);
        }
    }
}

#[test]
fn string_prefix_refusals_and_lock_invalidity_precede_conditional_execution() {
    for form in forms() {
        let opcode = &form[usize::from(form[0] == 0x66)..];
        for bytes in [
            [&[0xf2][..], form].concat(),
            [&[0xf3, 0xf3][..], form].concat(),
            [&[0xf2, 0xf3][..], form].concat(),
            [&[0xf3, 0xf2][..], form].concat(),
            [&[0x66, 0x66][..], opcode].concat(),
            [&[0x67, 0x67][..], form].concat(),
        ] {
            let memory = executable(&bytes);
            let strict = decode_one(&memory, GuestAddress(0x1000)).unwrap_err();
            assert!(matches!(strict, DecodeError::Unsupported(_)));
            assert_eq!(
                decode_for_execution(&memory, GuestAddress(0x1000)).unwrap_err(),
                strict
            );
        }
        for repeat in [&[][..], &[0xf3]] {
            let bytes = [&[0xf0][..], repeat, form].concat();
            let memory = executable(&bytes);
            assert_eq!(
                decode_one(&memory, GuestAddress(0x1000)).unwrap_err(),
                DecodeError::InvalidEncoding
            );
            let decoded = decode_for_execution(&memory, GuestAddress(0x1000)).unwrap();
            assert_eq!(
                *decoded.operation(),
                Operation::Control(ControlOperation::InvalidOpcode)
            );
            assert_eq!(decoded.segment_accesses(), 0);
        }
    }
}
