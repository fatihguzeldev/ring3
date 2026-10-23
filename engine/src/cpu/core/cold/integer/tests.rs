use super::*;
use crate::{
    cpu::x86::SegmentContext,
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

fn memory() -> AddressSpace {
    let mut memory = AddressSpace::new(2).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x3000), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    memory
}

fn run(
    operation: Operation,
    state: &mut State32,
    memory: &mut AddressSpace,
) -> Result<bool, Failure> {
    execute(
        operation,
        state,
        &mut Port {
            memory,
            segments: &SegmentContext::default(),
            code_written: false,
        },
    )
}

#[test]
fn cold_scalar_shift_and_signed_divide_need_no_compiled_owner() {
    let mut memory = memory();
    let mut state = State32 {
        registers: [0x8000_0001, 0, 0xffff_ffff, 3, 0, 0, 0, 0],
        eip: 0x1000,
        eflags: 0x403,
    };
    assert!(matches!(
        run(
            Operation::Shift {
                kind: ShiftKind::Shl,
                destination: Location32::Register(Register32::Eax),
                count: ShiftCount::Immediate(1)
            },
            &mut state,
            &mut memory
        ),
        Ok(true)
    ));
    assert_eq!((state.registers[0], state.eflags), (2, 0xc03));
    state.registers[0] = (-7_i32) as u32;
    state.registers[2] = u32::MAX;
    state.eflags = 0xcd7;
    assert!(matches!(
        run(
            Operation::DivideAccumulator {
                kind: DivideKind::Signed,
                source: Register32::Ebx
            },
            &mut state,
            &mut memory
        ),
        Ok(true)
    ));
    assert_eq!(
        (state.registers[0], state.registers[2], state.eflags),
        ((-2_i32) as u32, u32::MAX, 0xcd7)
    );
}

#[test]
fn zero_count_memory_shift_is_still_an_exact_write_transaction() {
    let mut memory = memory();
    memory
        .write(GuestAddress(0x3000), &0x1234_5678_u32.to_le_bytes())
        .unwrap();
    let mut state = State32 {
        registers: [0; 8],
        eip: 0x1000,
        eflags: 0xcd7,
    };
    let operation = Operation::Shift {
        kind: ShiftKind::Shr,
        destination: Location32::Memory(EffectiveAddress {
            displacement: 0x3000,
            ..Default::default()
        }),
        count: ShiftCount::Immediate(32),
    };
    let version = memory.transaction_version();
    assert!(matches!(run(operation, &mut state, &mut memory), Ok(true)));
    assert_eq!(state.eflags, 0xcd7);
    assert_eq!(memory.transaction_version(), version + 1);
    memory
        .protect(
            PageRange::new(GuestAddress(0x3000), 1).unwrap(),
            Permissions::READ,
        )
        .unwrap();
    let before = state;
    assert!(run(operation, &mut state, &mut memory).is_err());
    assert_eq!(state, before);
}

#[test]
fn signed_division_overflow_and_source_fault_preserve_all_inputs() {
    let mut memory = memory();
    let mut state = State32 {
        registers: [i32::MIN as u32, 0, u32::MAX, u32::MAX, 0, 0, 0, 0],
        eip: 0x1000,
        eflags: 0xcd7,
    };
    let before = state;
    assert!(
        run(
            Operation::DivideAccumulator {
                kind: DivideKind::Signed,
                source: Register32::Ebx
            },
            &mut state,
            &mut memory
        )
        .is_err()
    );
    assert_eq!(state, before);
    assert!(
        run(
            Operation::ReadDivideAccumulator {
                kind: DivideKind::Unsigned,
                address: EffectiveAddress {
                    displacement: 0x5000,
                    ..Default::default()
                }
            },
            &mut state,
            &mut memory
        )
        .is_err()
    );
    assert_eq!(state, before);
}

#[test]
fn negative_memory_bit_index_selects_the_preceding_word_and_only_cf_changes() {
    let mut memory = memory();
    memory
        .write(GuestAddress(0x3000), &0x8000_u16.to_le_bytes())
        .unwrap();
    let mut state = State32 {
        registers: [0, 0, 0, 0x3020, 0, 0, 0, u32::MAX],
        eip: 0x1000,
        eflags: 0xcd6,
    };
    assert!(matches!(
        run(
            Operation::ReadBitTestWord {
                address: EffectiveAddress {
                    displacement: 0x3002,
                    ..Default::default()
                },
                index: BitIndex::Register(Register32::Edi),
            },
            &mut state,
            &mut memory
        ),
        Ok(true)
    ));
    assert_eq!(state.eflags, 0xcd7);
    assert_eq!(state.registers[7], u32::MAX);
}

#[test]
fn width_count_and_carry_rotations_preserve_the_profile_flags() {
    let mut memory = memory();
    for (operation, input, flags, output, expected_flags) in [
        (
            Operation::ByteRotateImmediate {
                kind: RotateKind::Left,
                destination: ByteRegister::Al,
                count: 8,
            },
            0x1234_5681,
            0xcd6,
            0x1234_5681,
            0x4d7,
        ),
        (
            Operation::RotateThroughCarryWord {
                kind: RotateKind::Left,
                destination: Register32::Eax,
                count: ShiftCount::Immediate(17),
            },
            0x1234_8001,
            0xcd7,
            0x1234_8001,
            0xcd7,
        ),
        (
            Operation::ByteRotateThroughCarryOne {
                kind: RotateKind::Right,
                destination: ByteRegister::Al,
            },
            0x1234_5601,
            2,
            0x1234_5600,
            3,
        ),
        (
            Operation::ShiftByte {
                kind: ShiftKind::Sar,
                destination: ByteRegister::Al,
                count: ShiftCount::Immediate(8),
            },
            0x1234_5680,
            2,
            0x1234_56ff,
            0x87,
        ),
        (
            Operation::ShiftByte {
                kind: ShiftKind::Shr,
                destination: ByteRegister::Al,
                count: ShiftCount::Immediate(8),
            },
            0x1234_5680,
            0x803,
            0x1234_5600,
            0x46,
        ),
        (
            Operation::DoubleShiftWord {
                kind: DoubleShiftKind::Left,
                destination: Register32::Eax,
                source: Register32::Ebx,
                count: ShiftCount::Immediate(16),
            },
            0xabcd_1234,
            0x402,
            0xabcd_8001,
            0x482,
        ),
        (
            Operation::DoubleShiftWord {
                kind: DoubleShiftKind::Right,
                destination: Register32::Eax,
                source: Register32::Ebx,
                count: ShiftCount::Immediate(17),
            },
            0xabcd_1234,
            2,
            0xabcd_0000,
            0x46,
        ),
    ] {
        let mut state = State32 {
            registers: [input, 0, 0, 0x8001, 0, 0, 0, 0],
            eip: 0x1000,
            eflags: flags,
        };
        assert!(matches!(run(operation, &mut state, &mut memory), Ok(true)));
        assert_eq!((state.registers[0], state.eflags), (output, expected_flags));
    }
}

#[test]
fn multiply_widths_store_the_full_product_and_only_defined_profile_flags() {
    let mut memory = memory();
    for (operation, eax, ebx, edx, output_eax, output_edx) in [
        (
            Operation::ByteMultiplyAccumulator {
                kind: MultiplyKind::Unsigned,
                source: ByteRegister::Bl,
            },
            0x1234_00ff,
            2,
            0xaabb_ccdd,
            0x1234_01fe,
            0xaabb_ccdd,
        ),
        (
            Operation::WordMultiplyAccumulator {
                kind: MultiplyKind::Signed,
                source: Register32::Ebx,
            },
            0xabcd_8000,
            0xffff,
            0x7654_3210,
            0xabcd_8000,
            0x7654_0000,
        ),
        (
            Operation::WordMultiplyAccumulator {
                kind: MultiplyKind::Unsigned,
                source: Register32::Ebx,
            },
            0x1234_ffff,
            0xffff,
            0x7654_3210,
            0x1234_0001,
            0x7654_fffe,
        ),
        (
            Operation::SignedMultiply {
                destination: Register32::Eax,
                source: Location32::Register(Register32::Ebx),
                immediate: None,
            },
            0x4000_0000,
            4,
            0x7654_3210,
            0,
            0x7654_3210,
        ),
    ] {
        let mut state = State32 {
            registers: [eax, 0, edx, ebx, 0, 0, 0, 0],
            eip: 0x1000,
            eflags: 0xcd7,
        };
        assert!(matches!(run(operation, &mut state, &mut memory), Ok(true)));
        assert_eq!(
            (state.registers[0], state.registers[2], state.eflags),
            (output_eax, output_edx, 0xc03)
        );
    }
}

#[test]
fn byte_division_places_remainder_in_ah_and_overflow_is_de_without_changes() {
    let mut memory = memory();
    let operation = Operation::ByteDivideAccumulator {
        kind: DivideKind::Unsigned,
        source: ByteRegister::Bl,
    };
    let mut state = State32 {
        registers: [0x1234_0205, 0, 0xaabb_ccdd, 3, 0, 0, 0, 0],
        eip: 0x1000,
        eflags: 0xcd7,
    };
    assert!(matches!(run(operation, &mut state, &mut memory), Ok(true)));
    assert_eq!(
        (state.registers[0], state.registers[2], state.eflags),
        (0x1234_01ac, 0xaabb_ccdd, 0xcd7)
    );
    state.registers[0] = 0x1234_0301;
    let before = state;
    assert!(matches!(
        run(operation, &mut state, &mut memory),
        Err(Failure::CpuFault(0))
    ));
    assert_eq!(state, before);
    state.registers[3] = 0;
    let before = state;
    assert!(matches!(
        run(operation, &mut state, &mut memory),
        Err(Failure::CpuFault(0))
    ));
    assert_eq!(state, before);
}

#[test]
fn extension_and_flag_byte_transfers_keep_unwritten_bits() {
    let mut memory = memory();
    let mut state = State32 {
        registers: [0x1234_8080, 0xaabb_ccdd, 0x7654_3210, 0, 0, 0, 0, 0],
        eip: 0x1000,
        eflags: 0xcd7,
    };
    assert!(matches!(
        run(
            Operation::ExtendByteToWord {
                kind: ExtensionKind::Sign,
                destination: Register32::Ecx,
                source: ByteRegister::Ah
            },
            &mut state,
            &mut memory
        ),
        Ok(true)
    ));
    assert_eq!(state.registers[1], 0xaabb_ff80);
    assert!(matches!(
        run(
            Operation::SignExtendAccumulatorWord,
            &mut state,
            &mut memory
        ),
        Ok(true)
    ));
    assert_eq!(state.registers[0], 0x1234_ff80);
    assert!(matches!(
        run(Operation::SignExtendHighWord, &mut state, &mut memory),
        Ok(true)
    ));
    assert_eq!(state.registers[2], 0x7654_ffff);
    assert!(matches!(
        run(Operation::FlagsToAh, &mut state, &mut memory),
        Ok(true)
    ));
    assert_eq!(state.registers[0], 0x1234_d780);
    state.registers[0] &= !0xff00;
    assert!(matches!(
        run(Operation::AhToFlags, &mut state, &mut memory),
        Ok(true)
    ));
    assert_eq!(state.eflags, 0xc02);
}

#[test]
fn scans_preserve_zero_source_destination_and_tzcnt_has_distinct_cf_zf() {
    let mut memory = memory();
    let mut state = State32 {
        registers: [0x1234_abcd, 0, 0, 0, 0, 0, 0, 0],
        eip: 0x1000,
        eflags: 2,
    };
    assert!(matches!(
        run(
            Operation::BitScan {
                kind: BitScanKind::Forward,
                destination: Register32::Eax,
                source: Register32::Ebx
            },
            &mut state,
            &mut memory
        ),
        Ok(true)
    ));
    assert_eq!((state.registers[0], state.eflags), (0x1234_abcd, 0x42));
    assert!(matches!(
        run(
            Operation::TrailingZeroCount32 {
                destination: Register32::Eax,
                source: Register32::Ebx
            },
            &mut state,
            &mut memory
        ),
        Ok(true)
    ));
    assert_eq!((state.registers[0], state.eflags), (32, 3));
    state.registers[3] = 1;
    assert!(matches!(
        run(
            Operation::TrailingZeroCount32 {
                destination: Register32::Eax,
                source: Register32::Ebx
            },
            &mut state,
            &mut memory
        ),
        Ok(true)
    ));
    assert_eq!((state.registers[0], state.eflags), (0, 0x42));
}

#[test]
fn locked_logical_immediate_and_register_forms_share_checked_rmw_authority() {
    let mut memory = memory();
    let address = EffectiveAddress {
        displacement: 0x3000,
        ..Default::default()
    };
    let mut state = State32 {
        registers: [0, 0, 0, 0x8000_0000, 0, 0, 0, 0],
        eip: 0x1000,
        eflags: 0xcd7,
    };
    for (operation, input, output, expected_flags) in [
        (
            Operation::LockedMemoryOr32 {
                address,
                mask: 0x8000_0000,
            },
            0_u32,
            0x8000_0000_u32,
            0x486,
        ),
        (
            Operation::LockedMemoryAnd32 { address, mask: 0 },
            0x8000_0000,
            0,
            0x446,
        ),
        (
            Operation::LockedMemoryXor32 { address, mask: 1 },
            0x8000_0001,
            0x8000_0000,
            0x486,
        ),
        (
            Operation::LockedMemoryOrRegister32 {
                address,
                source: Register32::Ebx,
            },
            0,
            0x8000_0000,
            0x486,
        ),
        (
            Operation::LockedMemoryAndRegister32 {
                address,
                source: Register32::Ebx,
            },
            0x7fff_ffff,
            0,
            0x446,
        ),
        (
            Operation::LockedMemoryXorRegister32 {
                address,
                source: Register32::Ebx,
            },
            0x8000_0000,
            0,
            0x446,
        ),
    ] {
        memory
            .write(GuestAddress(0x3000), &input.to_le_bytes())
            .unwrap();
        let version = memory.transaction_version();
        assert!(matches!(run(operation, &mut state, &mut memory), Ok(true)));
        let mut value = [0; 4];
        memory.read(GuestAddress(0x3000), &mut value).unwrap();
        assert_eq!(u32::from_le_bytes(value), output);
        assert_eq!(state.eflags, expected_flags);
        assert_eq!(memory.transaction_version(), version + 1);
    }
    memory
        .protect(
            PageRange::new(GuestAddress(0x3000), 1).unwrap(),
            Permissions::READ,
        )
        .unwrap();
    let before = state;
    let version = memory.transaction_version();
    assert!(matches!(
        run(
            Operation::LockedMemoryOr32 { address, mask: 1 },
            &mut state,
            &mut memory
        ),
        Err(Failure::Memory(_, 4))
    ));
    assert_eq!(state, before);
    assert_eq!(memory.transaction_version(), version);
}
