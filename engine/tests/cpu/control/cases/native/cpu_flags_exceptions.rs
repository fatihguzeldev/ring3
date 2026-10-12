use ring3_engine::{
    cpu::dbt::{BlockSpec, CompileLimits, compile_entry_region, compile_region},
    cpu::x86::{
        ControlOperation, Register32,
        decode::{DecodeError, decode_one},
        ir::Operation,
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

fn memory(bytes: &[u8]) -> AddressSpace {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x1000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    memory.write(GuestAddress(0x1000), bytes).unwrap();
    memory
}

#[test]
fn control_ir_preserves_flag_width_exception_kind_and_bound_operand() {
    for (bytes, expected) in [
        (&[0x9d][..], ControlOperation::PopFlags { word: false }),
        (&[0x66, 0x9d][..], ControlOperation::PopFlags { word: true }),
        (&[0x0f, 0x0b][..], ControlOperation::InvalidOpcode),
        (&[0xcc][..], ControlOperation::Breakpoint),
        (&[0xce][..], ControlOperation::Overflow),
    ] {
        let memory = memory(bytes);
        assert_eq!(
            decode_one(&memory, GuestAddress(0x1000))
                .unwrap()
                .operation(),
            &Operation::Control(expected)
        );
    }
    for (bytes, word) in [(&[0x62, 0x03][..], false), (&[0x66, 0x62, 0x03][..], true)] {
        let memory = memory(bytes);
        let decoded = decode_one(&memory, GuestAddress(0x1000)).unwrap();
        let Operation::Control(ControlOperation::Bound {
            index,
            address,
            word: actual_word,
        }) = *decoded.operation()
        else {
            panic!("expected BOUND")
        };
        assert_eq!(index, Register32::Eax);
        assert_eq!(address.base, Some(Register32::Ebx));
        assert_eq!(actual_word, word);
    }
}

#[test]
fn flags_faults_and_traps_decode_in_the_32_bit_user_mode() {
    for bytes in [
        &[0x9d][..],
        &[0x66, 0x9d],
        &[0x0f, 0x0b],
        &[0xcc],
        &[0xce],
        &[0x62, 0x03],
        &[0x66, 0x62, 0x03],
        &[0x67, 0x62, 0x00],
        &[0x64, 0x62, 0x03],
        &[0x66, 0xcc],
    ] {
        let memory = memory(bytes);
        let decoded = decode_one(&memory, GuestAddress(0x1000))
            .unwrap_or_else(|error| panic!("{bytes:02x?}: {error:?}"));
        assert_eq!(usize::from(decoded.length()), bytes.len());
        assert_eq!(decoded.next_pc(), GuestAddress(0x1000 + bytes.len() as u32));
    }
}

#[test]
fn illegal_locked_controls_and_register_bound_are_invalid_encodings() {
    for bytes in [
        &[0xf0, 0x9d][..],
        &[0xf0, 0xcc],
        &[0xf0, 0xce],
        &[0xf0, 0x62, 0x03],
        &[0x62, 0xc0],
    ] {
        assert!(
            matches!(
                decode_one(&memory(bytes), GuestAddress(0x1000)),
                Err(DecodeError::InvalidEncoding)
            ),
            "{bytes:02x?}"
        );
    }
}

#[test]
fn unconditional_guest_exceptions_end_explicit_and_discovered_blocks() {
    for bytes in [&[0x0f, 0x0b][..], &[0xcc], &[0x66, 0xcc]] {
        let memory = memory(bytes);
        for compiled in [
            compile_region(
                &memory,
                &[BlockSpec {
                    entry: GuestAddress(0x1000),
                    byte_length: bytes.len() as u32,
                }],
                CompileLimits::default(),
            )
            .unwrap(),
            compile_entry_region(&memory, &[GuestAddress(0x1000)], CompileLimits::default())
                .unwrap(),
        ] {
            assert_eq!(compiled.metadata().instructions, 1);
            assert_eq!(
                &compiled.wasm_bytes(&memory).unwrap()[..8],
                b"\0asm\x01\0\0\0"
            );
        }
    }
}
