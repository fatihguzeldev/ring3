use super::*;
use crate::{
    cpu::x86::DefaultCpuReadPort32,
    memory::{AddressSpace, PageRange, Permissions},
};

fn ram(memory: &AddressSpace) -> [Vec<u8>; 2] {
    std::array::from_fn(|index| {
        let mut bytes = vec![0; 4096];
        memory
            .read(
                GuestAddress(if index == 0 { 0xffff_f000 } else { 0 }),
                &mut bytes,
            )
            .unwrap();
        bytes
    })
}

fn memory() -> AddressSpace {
    let mut memory = AddressSpace::new(2).unwrap();
    for address in [0xffff_f000, 0] {
        memory
            .map_zeroed(
                PageRange::new(GuestAddress(address), 1).unwrap(),
                Permissions::ALL,
            )
            .unwrap();
    }
    memory
}

#[test]
fn pushall_and_popall_keep_logical_b16_b32_pointers_while_one_slot_wraps_linear_memory() {
    for default32 in [true, false] {
        let mut memory = memory();
        let pointer = StackPointerSize::new(default32);
        let original = [
            1,
            2,
            3,
            4,
            if default32 { 0x2e } else { 0xabcd_002e },
            6,
            7,
            8,
        ];
        let pushed = prepare(
            &mut DefaultCpuReadPort32::new(&memory),
            0xffff_fff0,
            pointer,
            StackFrameKind::PushAll32,
            0,
            0,
            original,
        )
        .unwrap();
        assert_eq!(
            pushed.registers[4],
            if default32 { 0x0e } else { 0xabcd_000e }
        );
        assert_eq!(pushed.addresses[7], 0xffff_fffe);
        let version = memory.code_validation_version();
        memory
            .cpu_write_stack(
                &pushed.addresses[..pushed.count],
                &pushed.values[..pushed.count],
                4,
            )
            .unwrap();
        assert_eq!(memory.code_validation_version(), version + 1);
        let popped = prepare(
            &mut DefaultCpuReadPort32::new(&memory),
            0xffff_fff0,
            pointer,
            StackFrameKind::PopAll32,
            0,
            0,
            pushed.registers,
        )
        .unwrap();
        assert_eq!(popped.registers, original);
    }
}

#[test]
fn nested_enter_alias_read_observes_earlier_wrapped_push_without_publishing_before_commit() {
    let mut memory = memory();
    let original = [1, 2, 3, 4, 0x12, 0x12, 7, 8];
    let before = ram(&memory);
    let prepared = prepare(
        &mut DefaultCpuReadPort32::new(&memory),
        0xffff_fff0,
        StackPointerSize::new(true),
        StackFrameKind::Enter32,
        0,
        2,
        original,
    )
    .unwrap();
    assert_eq!(
        &prepared.addresses[..3],
        &[0xffff_fffe, 0xffff_fffa, 0xffff_fff6]
    );
    assert_eq!(&prepared.values[..3], &[0x12, 0x12, 0x0e]);
    assert_eq!((prepared.registers[4], prepared.registers[5]), (6, 0x0e));
    assert_eq!(ram(&memory), before);
    let version = memory.code_validation_version();
    memory
        .cpu_write_stack(&prepared.addresses[..3], &prepared.values[..3], 4)
        .unwrap();
    assert_eq!(memory.code_validation_version(), version + 1);
    let mut bytes = [0; 12];
    memory
        .cpu_read(GuestAddress(0xffff_fff6), &mut bytes)
        .unwrap();
    assert_eq!(bytes, [0x0e, 0, 0, 0, 0x12, 0, 0, 0, 0x12, 0, 0, 0]);
}
