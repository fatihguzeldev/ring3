#[path = "support/division_cases.rs"]
mod division_cases;
#[path = "support/executable.rs"]
mod executable;
#[path = "support/unsigned_division_executable.rs"]
mod unsigned_division_executable;

use ring3_core::execution::{
    Cpu32, Permissions, Process32, ProcessStop, Register32, StopReason, load_pe32,
};

#[test]
fn constructed_quotient_remainder_identities_hold_for_all_widths_and_operand_forms() {
    division_cases::arithmetic();
}

#[test]
fn zero_divisor_and_oversized_quotient_stop_without_cpu_changes() {
    division_cases::errors();
}

#[test]
fn unsigned_quotient_and_remainder_forms_run_whole_or_stepwise() {
    assert_eq!(
        unsigned_division_executable::encoding(32, 0xc3, None),
        [0xf7, 0xf3]
    );
    let mut expected = None;
    for budget in [1, 40] {
        let mut image = load_pe32(&unsigned_division_executable::pe32(), 16).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        let mut count = 0;
        loop {
            let run = cpu.run(&mut image.memory, budget);
            count += run.instructions;
            if run.reason != StopReason::InstructionLimit {
                assert_eq!(run.reason, StopReason::Breakpoint);
                break;
            }
            assert!(count < 40);
        }
        assert_eq!(count, 14);
        assert_eq!(cpu.register(Register32::Esi), 252_648_990);
        assert_eq!(cpu.register(Register32::Edi), 5);
        assert_eq!(cpu.register(Register32::Eax), 0xabcd_00ff);
        assert_eq!(cpu.register(Register32::Edx), 4);
        if let Some(expected) = expected {
            assert_eq!(cpu, expected);
        }
        expected = Some(cpu);
    }
}

#[test]
fn source_aliases_are_captured_before_either_destination_changes() {
    for (bits, source, eax, edx, result) in [
        (8, 0xc0, 0xabcd_0190, 77, Some((0xabcd_7002, 77))),
        (8, 0xc4, 0xabcd_0190, 77, None),
        (
            16,
            0xc0,
            0xabcd_0100,
            0xface_0000,
            Some((0xabcd_0001, 0xface_0000)),
        ),
        (16, 0xc2, 0xabcd_0100, 0xface_0001, None),
        (32, 0xc0, 0xffff_fffe, 0, Some((1, 0))),
        (32, 0xc2, 0, 17, None),
    ] {
        let code = unsigned_division_executable::encoding(bits, source, None);
        let mut image = load_pe32(&executable::pe32(&code), 16).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        cpu.set_register(Register32::Eax, eax);
        cpu.set_register(Register32::Edx, edx);
        let before = cpu;
        let run = cpu.run(&mut image.memory, 1);
        if let Some((low, high)) = result {
            assert_eq!(run.reason, StopReason::InstructionLimit);
            assert_eq!(cpu.register(Register32::Eax), low);
            assert_eq!(cpu.register(Register32::Edx), high);
        } else {
            assert_eq!(run.reason, StopReason::DivideError);
            assert_eq!(cpu, before);
        }
    }
}

#[test]
fn source_protection_cross_page_and_address_wrap_faults_are_retryable() {
    for bits in [8, 16, 32] {
        let width = bits / 8;
        for address in [0x6000_0000, if bits == 8 { 0x0040_3000 } else { u32::MAX }] {
            let code = unsigned_division_executable::encoding(bits, 5, Some(address));
            let mut image = load_pe32(&executable::pe32(&code), 16).unwrap();
            let mut cpu = Cpu32::new(image.entry_point);
            let before = cpu;
            assert!(matches!(
                cpu.run(&mut image.memory, 1).reason,
                StopReason::MemoryFault(_)
            ));
            assert_eq!(cpu, before);
        }
        let address = u32::MAX - width + 1;
        let mut code = vec![0x64];
        code.extend(unsigned_division_executable::encoding(
            bits,
            5,
            Some(address.wrapping_sub(0x1000)),
        ));
        let mut image = load_pe32(&executable::pe32(&code), 16).unwrap();
        image
            .memory
            .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
            .unwrap();
        image
            .memory
            .write(
                u64::from(address),
                &17_u32.to_le_bytes()[..usize::try_from(width).unwrap()],
            )
            .unwrap();
        image
            .memory
            .protect(0xffff_f000, 4096, Permissions::NONE)
            .unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        cpu.set_fs_base(0x1000);
        cpu.set_register(Register32::Eax, 100);
        let before = cpu;
        assert!(matches!(
            cpu.run(&mut image.memory, 1).reason,
            StopReason::MemoryFault(_)
        ));
        assert_eq!(cpu, before);
        image
            .memory
            .protect(0xffff_f000, 4096, Permissions::READ)
            .unwrap();
        assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
        assert_eq!(
            cpu.register(Register32::Eax),
            if bits == 8 { 0x0f05 } else { 5 }
        );
        if bits != 8 {
            assert_eq!(cpu.register(Register32::Edx), 15);
        }
    }
    let code = unsigned_division_executable::encoding(32, 5, Some(0x0040_2fff));
    let mut image = load_pe32(&executable::pe32(&code), 16).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_register(Register32::Eax, 100);
    let before = cpu;
    assert!(matches!(
        cpu.run(&mut image.memory, 1).reason,
        StopReason::MemoryFault(_)
    ));
    assert_eq!(cpu, before);
    image
        .memory
        .map_zeroed(0x0040_3000, 4096, Permissions::READ_WRITE)
        .unwrap();
    image
        .memory
        .write(0x0040_2fff, &17_u32.to_le_bytes())
        .unwrap();
    assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
    assert_eq!(cpu.register(Register32::Eax), 5);
    assert_eq!(cpu.register(Register32::Edx), 15);
}

#[test]
fn prefixes_and_truncated_instruction_fetch_fail_before_execution() {
    for (prefix, reason) in [
        (0xf2, StopReason::UnsupportedInstruction),
        (0xf3, StopReason::UnsupportedInstruction),
        (0x67, StopReason::UnsupportedInstruction),
        (0xf0, StopReason::InvalidInstruction),
    ] {
        let code = [prefix, 0xf7, 0x37];
        let mut image = load_pe32(&executable::pe32(&code), 16).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        let before = cpu;
        assert_eq!(cpu.run(&mut image.memory, 1).reason, reason);
        assert_eq!(cpu, before);
    }
    let mut image = load_pe32(&executable::pe32(&[]), 16).unwrap();
    image
        .memory
        .map_zeroed(0x5000_0000, 4096, Permissions::READ_WRITE)
        .unwrap();
    image.memory.write(0x5000_0fff, &[0xf7]).unwrap();
    image
        .memory
        .protect(0x5000_0000, 4096, Permissions::READ_EXECUTE)
        .unwrap();
    let mut cpu = Cpu32::new(0x5000_0fff);
    let before = cpu;
    assert!(matches!(
        cpu.run(&mut image.memory, 1).reason,
        StopReason::MemoryFault(_)
    ));
    assert_eq!(cpu, before);
}

#[test]
fn process_reports_divide_error_without_counting_a_failed_instruction_and_can_resume() {
    let mut p = Process32::load(&executable::pe32(&[0xf7, 0xf3, 0xcc]), 32).unwrap();
    p.cpu.set_register(Register32::Eax, 100);
    let before = p.cpu;
    let run = p.run(10);
    assert_eq!(run.reason, ProcessStop::Stopped(StopReason::DivideError));
    assert_eq!((run.instructions, run.api_calls), (0, 0));
    assert_eq!(p.cpu, before);
    p.cpu.set_register(Register32::Ebx, 17);
    assert_eq!(
        p.run(10).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(p.cpu.register(Register32::Eax), 5);
    assert_eq!(p.cpu.register(Register32::Edx), 15);
}
