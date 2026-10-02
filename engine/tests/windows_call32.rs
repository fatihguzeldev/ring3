use ring3_engine::cpu::x86::State32;
use ring3_engine::memory::{
    Access, AddressSpace, FaultReason, GuestAddress, MemoryError, MemoryFault, PageRange,
    Permissions,
};
use ring3_engine::windows::{CallFrame32, CallingConvention32, FrameError, MAX_STACK_WORDS};

fn state(esp: u32) -> State32 {
    State32 {
        registers: [
            0x1122_3344,
            0xfedc_ba98,
            0x2233_4455,
            0x3344_5566,
            esp,
            0x4455_6677,
            0x5566_7788,
            0x6677_8899,
        ],
        eip: 0x1234_5678,
        eflags: 0xcd7,
    }
}

fn map(memory: &mut AddressSpace, address: u32, pages: u32) {
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(address), pages).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
}

fn word(memory: &mut AddressSpace, address: u32, value: u32) {
    memory
        .write(GuestAddress(address), &value.to_le_bytes())
        .unwrap();
}

fn fault(address: u32, reason: FaultReason) -> FrameError {
    FrameError::Memory(MemoryError::Fault(MemoryFault {
        address: GuestAddress(address),
        access: Access::Read,
        reason,
    }))
}

#[test]
fn calling_convention_tags_are_closed_and_capacity_is_sixteen_words() {
    assert_eq!(MAX_STACK_WORDS, 16);
    for (tag, expected) in [
        (1, CallingConvention32::Cdecl),
        (2, CallingConvention32::Stdcall),
        (3, CallingConvention32::Thiscall),
    ] {
        assert_eq!(CallingConvention32::try_from(tag), Ok(expected));
    }
    for tag in [0, 4, u32::MAX] {
        assert_eq!(
            CallingConvention32::try_from(tag),
            Err(FrameError::InvalidRequest)
        );
    }
}

#[test]
fn all_conventions_capture_zero_one_and_sixteen_arguments_and_preserve_full_state() {
    for convention in [
        CallingConvention32::Cdecl,
        CallingConvention32::Stdcall,
        CallingConvention32::Thiscall,
    ] {
        for count in [0, 1, 16] {
            let mut memory = AddressSpace::new(1).unwrap();
            map(&mut memory, 0x8000, 1);
            word(&mut memory, 0x8000, 0xf123_4567);
            let expected_arguments: Vec<_> = (0..count).map(|index| 0x9876_5400 + index).collect();
            for (index, value) in expected_arguments.iter().enumerate() {
                word(&mut memory, 0x8004 + index as u32 * 4, *value);
            }
            let original = state(0x8000);
            let snapshot = memory
                .snapshot_code(GuestAddress(0x8000), 4 + count as usize * 4)
                .unwrap();
            let frame = CallFrame32::capture(&memory, original, convention, count).unwrap();
            assert_eq!(frame.state(), &original);
            assert_eq!(frame.convention(), convention);
            assert_eq!(frame.stack_words(), count);
            assert_eq!(frame.arguments(), expected_arguments);
            assert_eq!(frame.return_pc(), 0xf123_4567);
            assert_eq!(
                frame.this_pointer(),
                if convention == CallingConvention32::Thiscall {
                    Some(0xfedc_ba98)
                } else {
                    None
                }
            );
            let mut expected = original;
            expected.registers[0] = 0xdead_beef;
            expected.registers[4] = 0x8004
                + if convention == CallingConvention32::Cdecl {
                    0
                } else {
                    count * 4
                };
            expected.eip = 0xf123_4567;
            assert_eq!(frame.complete(0xdead_beef), expected);
            assert_eq!(frame.state(), &original);
            assert!(memory.is_code_current(&snapshot));
            let mut bytes = vec![0; 4 + count as usize * 4];
            memory.read(GuestAddress(0x8000), &mut bytes).unwrap();
            assert_eq!(&bytes[..4], &0xf123_4567u32.to_le_bytes());
            for (index, value) in expected_arguments.iter().enumerate() {
                assert_eq!(&bytes[4 + index * 4..8 + index * 4], &value.to_le_bytes());
            }
        }
    }
}

#[test]
fn zero_words_read_only_the_return_and_thiscall_does_not_dereference_ecx() {
    let mut memory = AddressSpace::new(1).unwrap();
    map(&mut memory, 0xffff_f000, 1);
    word(&mut memory, 0xffff_fffc, 0);
    for convention in [
        CallingConvention32::Cdecl,
        CallingConvention32::Stdcall,
        CallingConvention32::Thiscall,
    ] {
        let original = state(0xffff_fffc);
        let frame = CallFrame32::capture(&memory, original, convention, 0).unwrap();
        assert!(frame.arguments().is_empty());
        let mut expected = original;
        expected.registers[0] = u32::MAX;
        expected.registers[4] = 0;
        expected.eip = 0;
        assert_eq!(frame.complete(u32::MAX), expected);
    }
    assert!(memory.resolve(GuestAddress(0), Access::Read).is_err());
    assert!(
        memory
            .resolve(GuestAddress(0xfedc_ba98), Access::Read)
            .is_err()
    );
}

#[test]
fn unaligned_return_and_arguments_cross_pages_without_changing_argument_order() {
    let mut memory = AddressSpace::new(2).unwrap();
    map(&mut memory, 0x8000, 2);
    word(&mut memory, 0x8fff, 0x1020_3040);
    word(&mut memory, 0x9003, 0xdead_beef);
    word(&mut memory, 0x9007, 0x0123_4567);
    let original = state(0x8fff);
    let frame = CallFrame32::capture(&memory, original, CallingConvention32::Stdcall, 2).unwrap();
    assert_eq!(frame.return_pc(), 0x1020_3040);
    assert_eq!(frame.arguments(), &[0xdead_beef, 0x0123_4567]);
    let mut expected = original;
    expected.registers[0] = 0;
    expected.registers[4] = 0x900b;
    expected.eip = 0x1020_3040;
    assert_eq!(frame.complete(0), expected);
}

#[test]
fn argument_effective_addresses_and_completion_cleanup_wrap_at_thirty_two_bits() {
    let mut memory = AddressSpace::new(2).unwrap();
    map(&mut memory, 0xffff_f000, 1);
    map(&mut memory, 0, 1);
    word(&mut memory, 0xffff_fffc, 0x2345_6789);
    for index in 0..16 {
        word(&mut memory, index * 4, 0x7654_3200 + index);
    }
    for convention in [
        CallingConvention32::Cdecl,
        CallingConvention32::Stdcall,
        CallingConvention32::Thiscall,
    ] {
        let frame = CallFrame32::capture(&memory, state(0xffff_fffc), convention, 16).unwrap();
        assert_eq!(
            frame.arguments(),
            &(0..16).map(|index| 0x7654_3200 + index).collect::<Vec<_>>()
        );
        let completed = frame.complete(1);
        assert_eq!(
            completed.registers[4],
            if convention == CallingConvention32::Cdecl {
                0
            } else {
                64
            }
        );
        assert_eq!(completed.eip, 0x2345_6789);
    }
}

#[test]
fn invalid_argument_count_precedes_unmapped_return_or_argument_reads() {
    let memory = AddressSpace::new(1).unwrap();
    for count in [17, u32::MAX] {
        assert_eq!(
            CallFrame32::capture(
                &memory,
                state(0xffff_fffd),
                CallingConvention32::Thiscall,
                count
            ),
            Err(FrameError::InvalidRequest)
        );
    }
}

#[test]
fn return_read_and_each_argument_fault_are_checked_in_order_with_their_own_width() {
    let mut memory = AddressSpace::new(2).unwrap();
    map(&mut memory, 0, 1);
    word(&mut memory, 0, 0x1122_3344);
    assert_eq!(
        CallFrame32::capture(&memory, state(0xffff_fff8), CallingConvention32::Cdecl, 2),
        Err(fault(0xffff_fff8, FaultReason::Unmapped))
    );
    assert_eq!(
        CallFrame32::capture(&memory, state(0xffff_fffd), CallingConvention32::Cdecl, 1),
        Err(fault(0xffff_fffd, FaultReason::AddressOverflow))
    );
    map(&mut memory, 0xffff_f000, 1);
    word(&mut memory, 0xffff_fff9, 0x1234_5678);
    assert_eq!(
        CallFrame32::capture(&memory, state(0xffff_fff9), CallingConvention32::Cdecl, 2),
        Err(fault(0xffff_fffd, FaultReason::AddressOverflow))
    );
    assert_eq!(
        CallFrame32::capture(&memory, state(0xffff_fff9), CallingConvention32::Cdecl, 0)
            .unwrap()
            .return_pc(),
        0x1234_5678
    );
}

#[test]
fn later_unmapped_argument_keeps_the_successfully_read_prefix_and_code_stamps_unchanged() {
    let mut memory = AddressSpace::new(1).unwrap();
    map(&mut memory, 0xffff_f000, 1);
    word(&mut memory, 0xffff_fff8, 0x1234_5678);
    word(&mut memory, 0xffff_fffc, 0x8765_4321);
    let snapshot = memory.snapshot_code(GuestAddress(0xffff_fff8), 8).unwrap();
    let original = state(0xffff_fff8);
    assert_eq!(
        CallFrame32::capture(&memory, original, CallingConvention32::Stdcall, 2),
        Err(fault(0, FaultReason::Unmapped))
    );
    assert!(memory.is_code_current(&snapshot));
    let mut bytes = [0; 8];
    memory.read(GuestAddress(0xffff_fff8), &mut bytes).unwrap();
    assert_eq!(bytes, [0x78, 0x56, 0x34, 0x12, 0x21, 0x43, 0x65, 0x87]);
    assert_eq!(original, state(0xffff_fff8));
}

#[test]
fn permission_faults_keep_return_before_arguments_and_first_denied_page() {
    let mut memory = AddressSpace::new(2).unwrap();
    map(&mut memory, 0x8000, 2);
    word(&mut memory, 0x8ffe, 0x1234_5678);
    memory
        .protect(
            PageRange::new(GuestAddress(0x9000), 1).unwrap(),
            Permissions::from_bits(6).unwrap(),
        )
        .unwrap();
    assert_eq!(
        CallFrame32::capture(&memory, state(0x8ffe), CallingConvention32::Cdecl, 1),
        Err(fault(0x9000, FaultReason::Permission))
    );
    memory
        .protect(
            PageRange::new(GuestAddress(0x9000), 1).unwrap(),
            Permissions::ALL,
        )
        .unwrap();
    word(&mut memory, 0x8ffc, 0x1234_5678);
    word(&mut memory, 0x9000, 0xdead_beef);
    memory
        .protect(
            PageRange::new(GuestAddress(0x9000), 1).unwrap(),
            Permissions::from_bits(6).unwrap(),
        )
        .unwrap();
    let snapshot = memory.snapshot_code(GuestAddress(0x8ffc), 8).unwrap();
    assert_eq!(
        CallFrame32::capture(&memory, state(0x8ffc), CallingConvention32::Thiscall, 1),
        Err(fault(0x9000, FaultReason::Permission))
    );
    let mut bytes = [0; 8];
    memory.fetch(GuestAddress(0x8ffc), &mut bytes).unwrap();
    assert_eq!(bytes, [0x78, 0x56, 0x34, 0x12, 0xef, 0xbe, 0xad, 0xde]);
    assert!(memory.is_code_current(&snapshot));
}

#[test]
fn captured_frame_and_completion_survive_mutated_or_unmapped_guest_inputs() {
    let mut memory = AddressSpace::new(1).unwrap();
    map(&mut memory, 0x8000, 1);
    word(&mut memory, 0x8000, 0xf123_4567);
    word(&mut memory, 0x8004, 0xdead_beef);
    let original = state(0x8000);
    let frame = CallFrame32::capture(&memory, original, CallingConvention32::Thiscall, 1).unwrap();
    word(&mut memory, 0x8000, 0);
    word(&mut memory, 0x8004, 0);
    memory
        .unmap(PageRange::new(GuestAddress(0x8000), 1).unwrap())
        .unwrap();
    assert_eq!(frame.arguments(), &[0xdead_beef]);
    assert_eq!(frame.return_pc(), 0xf123_4567);
    assert_eq!(frame.this_pointer(), Some(0xfedc_ba98));
    let mut expected = original;
    expected.registers[0] = 0x7654_3210;
    expected.registers[4] = 0x8008;
    expected.eip = 0xf123_4567;
    assert_eq!(frame.complete(0x7654_3210), expected);
}
