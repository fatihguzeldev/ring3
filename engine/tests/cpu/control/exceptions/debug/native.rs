use ring3_engine::{
    cpu::{
        UnsupportedFeature,
        dbt::{BlockSpec, CompileLimits, compile_entry_region, compile_region},
        x86::{
            ControlOperation,
            decode::{DecodeError, decode_one},
            ir::Operation,
        },
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

fn code(bytes: &[u8]) -> AddressSpace {
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
fn int1_admits_as_a_distinct_memory_free_terminal_control() {
    for prefix in [&[][..], &[0x66], &[0x67], &[0x64], &[0x36]] {
        let mut bytes = prefix.to_vec();
        bytes.push(0xf1);
        let memory = code(&bytes);
        let instruction = decode_one(&memory, GuestAddress(0x1000)).unwrap();
        assert!(matches!(instruction.operation(), Operation::Control(_)));
        assert_ne!(
            instruction.operation(),
            &Operation::Control(ControlOperation::Breakpoint)
        );
        assert_eq!(instruction.length() as usize, bytes.len());
        assert_eq!(
            instruction.next_pc(),
            GuestAddress(0x1000 + bytes.len() as u32)
        );
        let Operation::Control(operation) = instruction.operation() else {
            unreachable!()
        };
        assert!(!operation.reads_memory());
        compile_region(
            &memory,
            &[BlockSpec {
                entry: GuestAddress(0x1000),
                byte_length: bytes.len() as u32,
            }],
            CompileLimits::default(),
        )
        .unwrap();
        compile_entry_region(&memory, &[GuestAddress(0x1000)], CompileLimits::default()).unwrap();
    }
}

#[test]
fn int1_ends_an_explicit_block_before_trailing_instructions() {
    let memory = code(&[0xf1, 0x90]);
    decode_one(&memory, GuestAddress(0x1000)).unwrap();
    assert!(
        compile_region(
            &memory,
            &[BlockSpec {
                entry: GuestAddress(0x1000),
                byte_length: 2
            }],
            CompileLimits::default(),
        )
        .is_err()
    );
}

#[test]
fn int1_does_not_admit_ordinary_interrupts_or_illegal_lock() {
    assert_eq!(
        decode_one(&code(&[0xcd, 0x01]), GuestAddress(0x1000)).unwrap_err(),
        DecodeError::Unsupported(UnsupportedFeature::Opcode)
    );
    assert_eq!(
        decode_one(&code(&[0xf0, 0xf1]), GuestAddress(0x1000)).unwrap_err(),
        DecodeError::InvalidEncoding
    );
}
