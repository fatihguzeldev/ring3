use ring3_engine::{
    cpu::{
        dbt::{
            BlockSpec, CompileError, CompileLimits, InstructionError, compile_entry_region,
            compile_region,
        },
        x86::{
            X87Operation, X87State,
            decode::{DecodeError, decode_one},
            ir::Operation,
        },
    },
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

const PC: u32 = 0x1234_1000;
const OPCODES: [u8; 3] = [0xe0, 0xe1, 0xe4];

fn code(bytes: &[u8]) -> AddressSpace {
    let mut memory = AddressSpace::new(1).unwrap();
    let page = PageRange::new(GuestAddress(PC & !0xfff), 1).unwrap();
    memory.map_zeroed(page, Permissions::ALL).unwrap();
    memory.write(GuestAddress(PC), bytes).unwrap();
    memory.protect(page, Permissions::EXECUTE).unwrap();
    memory
}

#[test]
fn raw_obsolete_opcodes_are_two_byte_integer_nops() {
    for opcode in OPCODES {
        let memory = code(&[0xdb, opcode]);
        let instruction = decode_one(&memory, GuestAddress(PC)).unwrap();
        assert_eq!(instruction.operation(), &Operation::Nop);
        assert_eq!(instruction.length(), 2);
        assert_eq!(instruction.next_pc(), GuestAddress(PC + 2));
        assert!(memory.is_code_current(instruction.code_snapshot()));
    }
}

#[test]
fn wait_aliases_keep_two_distinct_instructions() {
    for opcode in OPCODES {
        let memory = code(&[0x9b, 0xdb, opcode]);
        let wait = decode_one(&memory, GuestAddress(PC)).unwrap();
        assert!(matches!(
            wait.operation(),
            Operation::X87 {
                operation: X87Operation::Wait,
                address: None,
                waiting: true,
                opcode: 0,
            }
        ));
        assert_eq!(wait.length(), 1);
        assert_eq!(wait.next_pc(), GuestAddress(PC + 1));
        let nop = decode_one(&memory, wait.next_pc()).unwrap();
        assert_eq!(nop.operation(), &Operation::Nop);
        assert_eq!(nop.length(), 2);
        assert_eq!(nop.next_pc(), GuestAddress(PC + 3));
        assert!(memory.is_code_current(wait.code_snapshot()));
        assert!(memory.is_code_current(nop.code_snapshot()));
    }
}

#[test]
fn exact_and_discovered_regions_preserve_integer_and_wait_boundaries() {
    for opcode in OPCODES {
        for waiting in [false, true] {
            let bytes: Vec<_> = waiting
                .then_some(0x9b)
                .into_iter()
                .chain([0xdb, opcode, 0xeb, 0])
                .collect();
            let memory = code(&bytes);
            let exact = compile_region(
                &memory,
                &[BlockSpec {
                    entry: GuestAddress(PC),
                    byte_length: bytes.len() as u32,
                }],
                CompileLimits::default(),
            );
            let discovered =
                compile_entry_region(&memory, &[GuestAddress(PC)], CompileLimits::default());
            if waiting {
                // the existing x87 helper requires a bound backend; decode remains independent.
                assert_eq!(
                    exact.err(),
                    Some(CompileError::Instruction {
                        pc: GuestAddress(PC),
                        cause: InstructionError::BackendUnsupported,
                    })
                );
                assert_eq!(
                    discovered.err(),
                    Some(CompileError::Instruction {
                        pc: GuestAddress(PC),
                        cause: InstructionError::BackendUnsupported,
                    })
                );
                continue;
            }
            let exact = exact.unwrap();
            let discovered = discovered.unwrap();
            assert_eq!(exact.metadata().blocks, 1);
            assert_eq!(exact.metadata().instructions, 2);
            assert_eq!(exact.metadata(), discovered.metadata());
            assert_eq!(
                exact.wasm_bytes(&memory).unwrap(),
                discovered.wasm_bytes(&memory).unwrap()
            );
        }
    }
}

#[test]
fn existing_wait_retains_pending_exception_synchronization() {
    let state = X87State {
        control: 0x037e,
        status: 1,
        tag: 0x55aa,
        opcode: 0x7ff,
        instruction_pointer: 0xf1234567,
        data_pointer: 0xfedcba98,
        registers: std::array::from_fn(|r| std::array::from_fn(|b| (r * 19 + b * 29) as u8)),
        ..X87State::default()
    };
    let outcome = state.evaluate(X87Operation::Wait, &[], 0xcd7).unwrap();
    assert!(outcome.unmasked_exception);
    let expected = X87State {
        status: 0x8081,
        ..state
    };
    assert_eq!(outcome.state, expected);
    assert_eq!(outcome.memory_len, 0);
    assert_eq!(outcome.eflags, None);
    let masked = X87State {
        control: 0x037f,
        ..state
    };
    let outcome = masked.evaluate(X87Operation::Wait, &[], 0xcd7).unwrap();
    assert!(!outcome.unmasked_exception);
    assert_eq!(outcome.state, masked);
}

#[test]
fn lock_and_removed_coprocessor_neighbors_are_not_new_nops() {
    for opcode in OPCODES {
        assert_eq!(
            decode_one(&code(&[0xf0, 0xdb, opcode]), GuestAddress(PC)).unwrap_err(),
            DecodeError::InvalidEncoding
        );
    }
    for bytes in [[0xdb, 0xe5], [0xd9, 0xd1]] {
        assert!(decode_one(&code(&bytes), GuestAddress(PC)).is_err());
    }
}
