use ring3_engine::{
    cpu::x86::{X87Operation, X87State, decode::decode_one, ir::Operation},
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

fn operation(escape: u8, index: u8) -> X87Operation {
    let mut memory = AddressSpace::new(1).unwrap();
    let page = PageRange::new(GuestAddress(0x1000), 1).unwrap();
    memory.map_zeroed(page, Permissions::ALL).unwrap();
    memory
        .write(GuestAddress(0x1000), &[escape, 0xd8 + index])
        .unwrap();
    memory.protect(page, Permissions::EXECUTE).unwrap();
    let decoded = decode_one(&memory, GuestAddress(0x1000)).unwrap();
    let Operation::X87 {
        operation,
        address,
        waiting,
        ..
    } = *decoded.operation()
    else {
        panic!("expected a register x87 instruction")
    };
    assert!(waiting);
    assert_eq!(address, None);
    assert_eq!(decoded.length(), 2);
    assert_eq!(decoded.next_pc(), GuestAddress(0x1002));
    assert!(memory.is_code_current(decoded.code_snapshot()));
    assert_eq!(operation.read_size(), 0);
    assert_eq!(operation.write_size(), 0);
    assert_eq!(X87Operation::decode(operation.encode()), Some(operation));
    operation
}

fn empty(control: u16, top: usize) -> X87State {
    X87State {
        control,
        status: (top as u16) << 11,
        // empty register payloads are intentionally arbitrary; their result is not an oracle.
        registers: std::array::from_fn(|r| std::array::from_fn(|b| (r * 19 + b * 29) as u8)),
        ..X87State::default()
    }
}

fn no_fresh_underflow(control: u16) {
    for top in [0, 7] {
        for index in 0..8 {
            let state = empty(control, top);
            let outcome = state.evaluate(operation(0xd9, index), &[], 0xcd7).unwrap();
            assert_eq!(
                outcome.state.status & 0x41,
                0,
                "fstpnce must not raise fresh ie/sf: cw={control:04x} top={top} st({index})"
            );
            assert!(!outcome.unmasked_exception);
            assert_eq!(outcome.state.top(), (top + 1) & 7);
            assert_eq!(outcome.state.control, control);
            assert_eq!(outcome.memory_len, 0);
            assert_eq!(outcome.eflags, None);
            // the empty value and destination tag are architecturally unspecified here.
        }
    }
}

#[test]
fn masked_empty_source_pops_without_fresh_underflow() {
    no_fresh_underflow(0x037f);
}

#[test]
fn unmasked_empty_source_pops_without_fresh_underflow() {
    no_fresh_underflow(0x037e);
}

#[test]
fn nonempty_source_copies_raw80_then_pops_for_every_destination() {
    for top in [0, 7] {
        for index in 0..8 {
            let mut registers = [[0; 10]; 8];
            for (r, bytes) in registers.iter_mut().enumerate() {
                // a finite normal value whose low extended-precision bits survive the copy.
                let significand = 0x8000_0000_0000_0001_u64 | ((r as u64) << 4);
                bytes[..8].copy_from_slice(&significand.to_le_bytes());
                bytes[8..].copy_from_slice(&0x3fff_u16.to_le_bytes());
            }
            let state = X87State {
                status: (top as u16) << 11,
                tag: 0,
                registers,
                ..X87State::default()
            };
            let destination = (top + usize::from(index)) & 7;
            let mut expected = registers;
            expected[destination] = registers[top];
            let outcome = state.evaluate(operation(0xd9, index), &[], 0xcd7).unwrap();
            assert_eq!(outcome.state.registers, expected);
            assert_eq!(outcome.state.top(), (top + 1) & 7);
            assert_eq!(outcome.state.tag, 3 << (top * 2));
            assert_eq!(outcome.state.status & 0x41, 0);
            assert!(!outcome.unmasked_exception);
            assert_eq!(outcome.memory_len, 0);
            assert_eq!(outcome.eflags, None);
        }
    }
}

#[test]
fn waiting_pending_exception_precedes_copy_or_pop() {
    for index in 0..8 {
        let state = X87State {
            status: (7 << 11) | 1,
            ..empty(0x037e, 7)
        };
        let instruction = operation(0xd9, index);
        assert!(!instruction.ignores_pending_exception());
        let outcome = state.evaluate(instruction, &[], 0xcd7).unwrap();
        assert!(outcome.unmasked_exception);
        assert_eq!(
            outcome.state,
            X87State {
                status: state.status | 0x8080,
                ..state
            }
        );
        assert_eq!(outcome.memory_len, 0);
        assert_eq!(outcome.eflags, None);
    }
}

#[test]
fn ordinary_fstp_keeps_masked_and_unmasked_stack_underflow() {
    for control in [0x037f, 0x037e] {
        for index in 0..8 {
            let state = empty(control, 7);
            let outcome = state.evaluate(operation(0xdd, index), &[], 0xcd7).unwrap();
            assert_eq!(outcome.state.status & 0x41, 0x41);
            assert_eq!(outcome.unmasked_exception, control == 0x037e);
            assert_eq!(outcome.state.top(), if control == 0x037e { 7 } else { 0 });
            assert_eq!(outcome.memory_len, 0);
            assert_eq!(outcome.eflags, None);
        }
    }
}

#[test]
fn strict_descriptor_and_masked_sticky_underflow_remain_distinct() {
    for index in 0..8 {
        let instruction = X87Operation::StoreRegisterNoUnderflow { index };
        let descriptor = 38 | (u32::from(index) << 8);
        assert_eq!(operation(0xd9, index), instruction);
        assert_eq!(instruction.encode(), descriptor);
        assert_eq!(X87Operation::decode(descriptor), Some(instruction));
        assert_eq!(instruction.read_size(), 0);
        assert_eq!(instruction.write_size(), 0);
        assert!(!instruction.ignores_pending_exception());
        assert_eq!(X87Operation::decode(descriptor | (1 << 16)), None);
        assert_eq!(X87Operation::decode(descriptor | (1 << 24)), None);

        let state = X87State {
            status: (7 << 11) | 0x41,
            ..empty(0x037f, 7)
        };
        let outcome = state.evaluate(instruction, &[], 0xcd7).unwrap();
        assert_eq!(outcome.state.status & 0x80c1, 0x41);
        assert_eq!(outcome.state.top(), 0);
        assert!(!outcome.unmasked_exception);
        assert_eq!(outcome.memory_len, 0);
        assert_eq!(outcome.eflags, None);
    }
    for index in [8_u8, 0xff] {
        assert_eq!(X87Operation::decode(38 | (u32::from(index) << 8)), None);
    }
}
