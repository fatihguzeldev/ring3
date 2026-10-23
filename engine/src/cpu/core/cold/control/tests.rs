use super::*;
use crate::{
    cpu::x86::SegmentContext,
    memory::{AddressSpace, GuestAddress, PageRange, Permissions},
};

fn memory() -> AddressSpace {
    let mut memory = AddressSpace::new(1).unwrap();
    memory
        .map_zeroed(
            PageRange::new(GuestAddress(0x4000), 1).unwrap(),
            Permissions::READ_WRITE,
        )
        .unwrap();
    memory
}

fn state() -> State32 {
    State32 {
        registers: [0; 8],
        eip: 0x1000,
        eflags: 2,
    }
}

#[test]
fn pop_flags_uses_shared_masks_and_reports_completed_debug() {
    for word in [false, true] {
        let mut memory = memory();
        memory
            .write(GuestAddress(0x4000), &u32::MAX.to_le_bytes())
            .unwrap();
        let mut state = state();
        state.registers[4] = 0x4000;
        state.eflags = 0x250002;
        let segments = SegmentContext::default();
        let mut port = Port {
            memory: &mut memory,
            segments: &segments,
            code_written: false,
        };
        let op = Operation::Control(ControlOperation::PopFlags { word });
        assert!(execute(op, 0x1001, &mut state, &mut port).unwrap());
        assert_eq!(state.eflags, 0x244dd7);
        assert_eq!(state.registers[4], if word { 0x4002 } else { 0x4004 });
        assert!(matches!(
            completed_stop(op, &state),
            Some(Ok(StopReason::Debug))
        ));
    }
}

#[test]
fn terminal_and_completed_controls_do_not_confuse_fault_with_retirement() {
    let mut state = state();
    assert!(matches!(
        terminal(Operation::Control(ControlOperation::InvalidOpcode), &state),
        Some(Ok(StopReason::Unsupported))
    ));
    assert!(matches!(
        terminal(
            Operation::Control(ControlOperation::GeneralProtectionZero),
            &state
        ),
        Some(Err(Failure::CpuFault(13)))
    ));
    assert!(matches!(
        completed_stop(Operation::Control(ControlOperation::Breakpoint), &state),
        Some(Ok(StopReason::Debug))
    ));
    assert!(completed_stop(Operation::Control(ControlOperation::Overflow), &state).is_none());
    state.eflags |= 0x800;
    assert!(matches!(
        completed_stop(Operation::Control(ControlOperation::Overflow), &state),
        Some(Err(Failure::CpuFault(4)))
    ));
}

#[test]
fn denied_port_repeat_uses_selected_count_without_memory_or_device_access() {
    let mut state = state();
    state.registers[1] = 0x1234_0000;
    let word = Operation::Control(ControlOperation::DeniedPortString {
        repeat_count: Some(AddressSize::Bits16),
    });
    let dword = Operation::Control(ControlOperation::DeniedPortString {
        repeat_count: Some(AddressSize::Bits32),
    });
    assert!(terminal(word, &state).is_none());
    assert!(matches!(
        terminal(dword, &state),
        Some(Err(Failure::CpuFault(13)))
    ));
    let mut memory = memory();
    let version = memory.transaction_version();
    let segments = SegmentContext::default();
    let mut port = Port {
        memory: &mut memory,
        segments: &segments,
        code_written: false,
    };
    let before = state;
    assert!(execute(word, 0x1002, &mut state, &mut port).unwrap());
    assert_eq!(state, before);
    assert_eq!(memory.transaction_version(), version);
}

#[test]
fn bounds_are_signed_and_the_complete_pair_is_read_before_range_check() {
    let mut memory = memory();
    memory
        .write(GuestAddress(0x4000), &(-3_i32).to_le_bytes())
        .unwrap();
    memory
        .write(GuestAddress(0x4004), &2_i32.to_le_bytes())
        .unwrap();
    let mut state = state();
    state.registers[0] = (-2_i32) as u32;
    let address = EffectiveAddress {
        displacement: 0x4000,
        ..Default::default()
    };
    let op = Operation::Control(ControlOperation::Bound {
        index: Register32::Eax,
        address,
        word: false,
    });
    let segments = SegmentContext::default();
    let mut port = Port {
        memory: &mut memory,
        segments: &segments,
        code_written: false,
    };
    assert!(execute(op, 0x1002, &mut state, &mut port).unwrap());
    state.registers[0] = 3;
    assert!(matches!(
        execute(op, 0x1002, &mut state, &mut port),
        Err(Failure::CpuFault(5))
    ));
    let cross = Operation::Control(ControlOperation::Bound {
        index: Register32::Eax,
        address: EffectiveAddress {
            displacement: 0x4ffc,
            ..Default::default()
        },
        word: false,
    });
    assert!(matches!(
        execute(cross, 0x1002, &mut state, &mut port),
        Err(Failure::Memory(_, 8))
    ));
}
