use super::computer_name_executable;

use ring3_core::execution::{
    Cpu32, PAGE_SIZE, Permissions, Process32, ProcessStop, Register32, StopReason,
};

const API: u32 = 0x7000_020c;
const STACK: u32 = 0x1000_ff00;
const SIZE: u32 = 0x0040_2280;
const OUTPUT: u32 = 0x0040_22a0;
const ERROR: u32 = 0x7ffd_e034;

fn load() -> Process32 {
    Process32::load(&computer_name_executable::pe32(), 32).unwrap()
}

fn put(p: &mut Process32, address: u32, value: u32) {
    p.memory
        .write(u64::from(address), &value.to_le_bytes())
        .unwrap();
}

fn word(p: &Process32, address: u32) -> u32 {
    let mut bytes = [0; 4];
    p.memory.read(u64::from(address), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

fn prepare(p: &mut Process32, output: u32, size: u32) -> Cpu32 {
    p.cpu.eip = API;
    p.cpu.set_register(Register32::Esp, STACK);
    p.cpu.set_register(Register32::Eax, 99);
    p.cpu.eflags = 0xced7;
    put(p, STACK, 0x0040_1000);
    put(p, STACK + 4, output);
    put(p, STACK + 8, size);
    p.cpu
}

fn completed(p: &mut Process32, mut expected: Cpu32, value: u32, target: u32) {
    let result = p.run(1);
    assert_eq!(
        result.reason,
        ProcessStop::Stopped(StopReason::InstructionLimit)
    );
    assert_eq!((result.instructions, result.api_calls), (0, 1));
    expected.eip = target;
    expected.set_register(Register32::Esp, STACK + 12);
    expected.set_register(Register32::Eax, value);
    assert_eq!(p.cpu, expected);
}

fn fault(p: &mut Process32, expected: Cpu32) {
    let result = p.run(1);
    assert!(matches!(
        result.reason,
        ProcessStop::Stopped(StopReason::MemoryFault(_))
    ));
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(p.cpu, expected);
}

#[test]
fn capacity_controls_name_length_and_error_without_touching_extra_bytes() {
    let mut p = load();
    for capacity in [0, 1, 5, 6, 7, u32::MAX] {
        put(&mut p, SIZE, capacity);
        put(&mut p, ERROR, 77);
        p.memory.write(u64::from(OUTPUT), &[0x55; 10]).unwrap();
        let before = prepare(&mut p, OUTPUT, SIZE);
        let fits = capacity >= 6;
        completed(&mut p, before, u32::from(fits), 0x0040_1000);
        assert_eq!(word(&p, SIZE), if fits { 5 } else { 6 });
        assert_eq!(p.last_error().unwrap(), if fits { 77 } else { 111 });
        let mut bytes = [0; 10];
        p.memory.read(u64::from(OUTPUT), &mut bytes).unwrap();
        let mut expected = [0x55; 10];
        if fits {
            expected[..6].copy_from_slice(b"RING3\0");
        }
        assert_eq!(bytes, expected);
    }
    for output in [0, u32::MAX, 0x0040_1000] {
        put(&mut p, SIZE, 0);
        let before = prepare(&mut p, output, SIZE);
        completed(&mut p, before, 0, 0x0040_1000);
        assert_eq!(word(&p, SIZE), 6);
    }
}

#[test]
fn all_writes_are_checked_before_mutation_and_success_needs_no_error_access() {
    for output in [0, 0x0040_1000, 0x0040_2ffc, u32::MAX - 4] {
        let mut p = load();
        put(&mut p, SIZE, 6);
        put(&mut p, ERROR, 77);
        p.memory.write(0x0040_2ffc, &[0x55; 4]).unwrap();
        let before = prepare(&mut p, output, SIZE);
        fault(&mut p, before);
        assert_eq!(word(&p, SIZE), 6);
        assert_eq!(p.last_error().unwrap(), 77);
        assert_eq!(word(&p, 0x0040_2ffc), 0x5555_5555);
    }
    for size in [0, 0x0040_1000, 0x0040_2ffe, u32::MAX - 2] {
        let mut p = load();
        p.memory.write(u64::from(OUTPUT), &[0x55; 6]).unwrap();
        let before = prepare(&mut p, OUTPUT, size);
        fault(&mut p, before);
        assert_eq!(word(&p, OUTPUT), 0x5555_5555);
    }
    let mut p = load();
    put(&mut p, SIZE, 0);
    p.memory
        .protect(0x7ffd_e000, PAGE_SIZE, Permissions::NONE)
        .unwrap();
    let before = prepare(&mut p, OUTPUT, SIZE);
    fault(&mut p, before);
    assert_eq!(word(&p, SIZE), 0);
    put(&mut p, SIZE, 6);
    let before = prepare(&mut p, OUTPUT, SIZE);
    completed(&mut p, before, 1, 0x0040_1000);
    assert_eq!(word(&p, SIZE), 5);
}

#[test]
fn writable_spans_cross_pages_and_end_at_the_top_of_guest32() {
    let mut p = load();
    p.memory
        .map_zeroed(0x0040_3000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    p.memory
        .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    for (output, size) in [
        (0x0040_2ffd, SIZE),
        (u32::MAX - 5, SIZE),
        (OUTPUT, u32::MAX - 3),
    ] {
        put(&mut p, size, u32::MAX);
        let before = prepare(&mut p, output, size);
        completed(&mut p, before, 1, 0x0040_1000);
        let mut bytes = [0; 6];
        p.memory.read(u64::from(output), &mut bytes).unwrap();
        assert_eq!(&bytes, b"RING3\0");
        assert_eq!(word(&p, size), 5);
    }
    p.memory
        .protect(
            0xffff_f000,
            PAGE_SIZE,
            Permissions {
                write: true,
                ..Permissions::NONE
            },
        )
        .unwrap();
    put(&mut p, SIZE, 6);
    let before = prepare(&mut p, u32::MAX - 5, SIZE);
    completed(&mut p, before, 1, 0x0040_1000);
}

#[test]
fn aliases_follow_captured_capacity_and_ordered_outputs() {
    let mut p = load();
    for (output, size) in [
        (SIZE, SIZE),
        (STACK, SIZE),
        (STACK + 4, SIZE),
        (ERROR, SIZE),
        (OUTPUT, ERROR),
    ] {
        put(&mut p, size, 6);
        let before = prepare(&mut p, output, size);
        completed(
            &mut p,
            before,
            1,
            if output == STACK {
                0x474e_4952
            } else {
                0x0040_1000
            },
        );
        assert_eq!(word(&p, size), 5);
        if output != size {
            assert_eq!(word(&p, output), 0x474e_4952);
        }
    }
    put(&mut p, ERROR, 0);
    let before = prepare(&mut p, 0, ERROR);
    completed(&mut p, before, 0, 0x0040_1000);
    assert_eq!(word(&p, ERROR), 111);
}

#[test]
fn budget_and_complete_frame_precede_memory_access() {
    let mut p = load();
    let before = prepare(&mut p, 0, 0);
    let result = p.run(0);
    assert_eq!((result.instructions, result.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    p.cpu.set_register(Register32::Esp, 0x1000_fff8);
    let before = p.cpu;
    fault(&mut p, before);
}

#[test]
fn imported_guest_agrees_whole_and_single_step() {
    for budget in [1, 50] {
        let mut p = load();
        let (mut instructions, mut calls) = (0, 0);
        loop {
            let result = p.run(budget);
            instructions += result.instructions;
            calls += result.api_calls;
            if result.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                break;
            }
            assert!(instructions + calls < 50);
        }
        assert_eq!((instructions, calls), (12, 2));
        for (register, expected) in [
            (Register32::Eax, 1),
            (Register32::Ebx, 0),
            (Register32::Ecx, 6),
            (Register32::Edx, 5),
            (Register32::Esi, 0x474e_4952),
            (Register32::Esp, 0x1001_0000),
        ] {
            assert_eq!(p.cpu.register(register), expected);
        }
    }
}
