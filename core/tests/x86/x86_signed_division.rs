use super::executable;
use super::signed_division_cases;

use ring3_core::execution::{Permissions, Process32, ProcessStop, Register32, StopReason};
use signed_division_cases::{load, set_dividend};

#[test]
fn signed_quotients_remainders_and_constructive_identities_match_across_budgets() {
    signed_division_cases::arithmetic();
}

#[test]
fn zero_divisors_and_quotient_overflow_preserve_the_complete_cpu() {
    signed_division_cases::errors();
}

#[test]
fn aliased_sources_are_read_before_writes_and_divide_errors_can_be_repaired() {
    for (source, dividend, quotient, remainder) in [
        (0xf8, 71, 1_i32, 0_i32),
        (0xf8, -71, 1, 0),
        (0xfa, -71, 71, 0),
        (0xfe, 71, 8, 7),
    ] {
        let (mut cpu, mut memory) = load(&[0xf7, source], dividend, 8);
        let mut expected = cpu;
        expected.eip += 2;
        expected.set_register(Register32::Eax, quotient.cast_unsigned());
        expected.set_register(Register32::Edx, remainder.cast_unsigned());
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        assert_eq!(cpu, expected);
    }
    for (dividend, divisor) in [(71, 0), (i64::MIN, -1), (2_147_483_648, 1)] {
        let (mut cpu, mut memory) = load(&[0xf7, 0xfe], dividend, divisor);
        let before = cpu;
        assert_eq!(cpu.run(&mut memory, 1).reason, StopReason::DivideError);
        assert_eq!(cpu, before);
        set_dividend(&mut cpu, -71);
        cpu.set_register(Register32::Esi, 8);
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        assert_eq!(cpu.register(Register32::Eax), (-8_i32).cast_unsigned());
        assert_eq!(cpu.register(Register32::Edx), (-7_i32).cast_unsigned());
    }
}

#[test]
fn source_read_faults_precede_arithmetic_and_cross_page_sources_can_be_repaired() {
    let address = 0x5000_0ffe_u32;
    let mut code = vec![0xf7, 0x3d];
    code.extend(address.to_le_bytes());
    let (mut cpu, mut memory) = load(&code, i64::MIN, 0);
    let before = cpu;
    for permissions in [None, Some(Permissions::NONE), Some(Permissions::READ_WRITE)] {
        if let Some(permissions) = permissions {
            if permissions == Permissions::NONE {
                memory.map_zeroed(0x5000_0000, 4096, permissions).unwrap();
            } else {
                memory.protect(0x5000_0000, 4096, permissions).unwrap();
            }
        }
        let run = cpu.run(&mut memory, 1);
        assert!(matches!(run.reason, StopReason::MemoryFault(_)));
        assert_eq!(run.instructions, 0);
        assert_eq!(cpu, before);
    }
    memory
        .map_zeroed(0x5000_1000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(cpu.run(&mut memory, 1).reason, StopReason::DivideError);
    assert_eq!(cpu, before);
    memory
        .write(u64::from(address), &(-8_i32).to_le_bytes())
        .unwrap();
    memory
        .protect(0x5000_0000, 8192, Permissions::READ)
        .unwrap();
    set_dividend(&mut cpu, -71);
    assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
    assert_eq!(cpu.register(Register32::Eax), 8);
    assert_eq!(cpu.register(Register32::Edx), (-7_i32).cast_unsigned());
    let mut after = [0; 4];
    memory.read(u64::from(address), &mut after).unwrap();
    assert_eq!(after, (-8_i32).to_le_bytes());
}

#[test]
fn fs_address_wrap_and_last_dword_work_but_operand_range_cannot_wrap() {
    for address in [0xffff_fffc_u32, 0x5000_0000, 0xffff_fffd] {
        let mut code = vec![0x64, 0xf7, 0x3d];
        code.extend(address.wrapping_sub(0x6000_0000).to_le_bytes());
        let (mut cpu, mut memory) = load(&code, -71, 0);
        cpu.set_fs_base(0x6000_0000);
        let page = address & !0xfff;
        memory
            .map_zeroed(u64::from(page), 4096, Permissions::READ_WRITE)
            .unwrap();
        if address == 0xffff_fffd {
            let before = cpu;
            assert!(matches!(
                cpu.run(&mut memory, 1).reason,
                StopReason::MemoryFault(_)
            ));
            assert_eq!(cpu, before);
        } else {
            memory
                .write(u64::from(address), &8_i32.to_le_bytes())
                .unwrap();
            memory
                .protect(u64::from(page), 4096, Permissions::READ)
                .unwrap();
            let mut expected = cpu;
            expected.eip += u32::try_from(code.len()).unwrap();
            expected.set_register(Register32::Eax, (-8_i32).cast_unsigned());
            expected.set_register(Register32::Edx, (-7_i32).cast_unsigned());
            assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
            assert_eq!(cpu, expected);
        }
    }
}

#[test]
fn unsupported_widths_prefixes_and_truncated_fetch_preserve_state() {
    for (code, reason) in [
        (&[0xf6, 0xfe][..], StopReason::UnsupportedInstruction),
        (&[0x66, 0xf7, 0xfe][..], StopReason::UnsupportedInstruction),
        (&[0xf2, 0xf7, 0xfe][..], StopReason::UnsupportedInstruction),
        (&[0xf3, 0xf7, 0xfe][..], StopReason::UnsupportedInstruction),
        (&[0x67, 0xf7, 0x3f][..], StopReason::UnsupportedInstruction),
        (&[0xf0, 0xf7, 0x3f][..], StopReason::InvalidInstruction),
        (&[0x65, 0xf7, 0x3f][..], StopReason::UnsupportedInstruction),
    ] {
        let (mut cpu, mut memory) = load(code, 71, 8);
        let before = cpu;
        let run = cpu.run(&mut memory, 1);
        assert_eq!(run.reason, reason);
        assert_eq!(run.instructions, 0);
        assert_eq!(cpu, before);
    }
    let (mut cpu, mut memory) = load(&[0x67, 0xf7, 0xfe], 71, 8);
    assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
    assert_eq!(cpu.register(Register32::Eax), 8);
    memory
        .map_zeroed(0x5000_0000, 4096, Permissions::READ_WRITE)
        .unwrap();
    memory.write(0x5000_0fff, &[0xf7]).unwrap();
    memory
        .protect(0x5000_0000, 4096, Permissions::READ_EXECUTE)
        .unwrap();
    cpu.eip = 0x5000_0fff;
    let before = cpu;
    assert!(matches!(
        cpu.run(&mut memory, 1).reason,
        StopReason::MemoryFault(_)
    ));
    assert_eq!(cpu, before);
}

#[test]
fn process_accounts_for_success_and_retry_without_counting_failed_division() {
    for budget in [1, 10] {
        let mut process = Process32::load(&executable::pe32(&[0xf7, 0xfe, 0xcc]), 32).unwrap();
        set_dividend(&mut process.cpu, -71);
        let before = process.cpu;
        let run = process.run(budget);
        assert_eq!(run.reason, ProcessStop::Stopped(StopReason::DivideError));
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
        process.cpu.set_register(Register32::Esi, 8);
        let mut steps = 0;
        loop {
            let run = process.run(budget);
            steps += run.instructions;
            assert_eq!(run.api_calls, 0);
            if run.reason == ProcessStop::Stopped(StopReason::Breakpoint) {
                break;
            }
            assert_eq!(
                run.reason,
                ProcessStop::Stopped(StopReason::InstructionLimit)
            );
            assert!(steps < 2);
        }
        assert_eq!(steps, 2);
        assert_eq!(
            process.cpu.register(Register32::Eax),
            (-8_i32).cast_unsigned()
        );
        assert_eq!(
            process.cpu.register(Register32::Edx),
            (-7_i32).cast_unsigned()
        );
    }
}
