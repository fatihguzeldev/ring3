use super::imported_executable;
use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

const CODE: u32 = 0x0040_1000;
const API: u32 = 0x7000_01e4;
const LOWER: u32 = 0x0040_2180;
const NUMERATOR: u32 = LOWER + 8;
const DIVISOR: u32 = LOWER + 16;
const RESULT: u32 = LOWER + 24;
const LOWER_RESULT: u32 = LOWER + 32;

fn executable() -> Vec<u8> {
    let mut code = Vec::new();
    for address in [LOWER, NUMERATOR, DIVISOR] {
        code.extend([0xdd, 0x05]);
        code.extend(address.to_le_bytes());
    }
    code.extend([0xff, 0x15, 0x60, 0x20, 0x40, 0]);
    for address in [RESULT, LOWER_RESULT] {
        code.extend([0xdd, 0x1d]);
        code.extend(address.to_le_bytes());
    }
    code.push(0xcc);
    imported_executable::pe32(&code, "MSVCRT.dll", &["_CIfmod"])
}

fn read(process: &Process32, address: u32) -> u64 {
    let mut bytes = [0; 8];
    process.memory.read(u64::from(address), &mut bytes).unwrap();
    u64::from_le_bytes(bytes)
}

pub fn finite_results_across_budgets() {
    for (numerator, divisor, expected) in [
        (7.0_f64, 2.0_f64, 1.0_f64),
        (-7.0, 2.0, -1.0),
        (7.0, -2.0, 1.0),
        (-7.0, -2.0, -1.0),
        (5.75, 2.5, 0.75),
        (-5.75, 2.5, -0.75),
        (2.0, 4.0, 2.0),
        (-2.0, 4.0, -2.0),
        (6.0, 3.0, 0.0),
        (-6.0, 3.0, -0.0),
        (0.0, 3.0, 0.0),
        (-0.0, 3.0, -0.0),
    ] {
        for budget in [1, 3, 30] {
            let mut process = Process32::load(&executable(), 40).unwrap();
            for (address, value) in [
                (LOWER, 42.5_f64),
                (NUMERATOR, numerator),
                (DIVISOR, divisor),
            ] {
                process
                    .memory
                    .write(u64::from(address), &value.to_le_bytes())
                    .unwrap();
            }
            process.cpu.set_register(Register32::Eax, 0x1234_5678);
            process.cpu.eflags = 0x246;
            let stack = process.cpu.register(Register32::Esp);
            let mut counts = (0, 0);
            loop {
                let run = process.run(budget);
                counts.0 += run.instructions;
                counts.1 += run.api_calls;
                if run.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                    assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                    break;
                }
                assert!(counts.0 + counts.1 < 20);
            }
            assert_eq!(counts, (7, 1));
            assert_eq!(process.cpu.register(Register32::Esp), stack);
            assert_eq!(process.cpu.register(Register32::Eax), 0x1234_5678);
            assert_eq!(process.cpu.eflags, 0x246);
            assert_eq!(read(&process, RESULT), expected.to_bits());
            assert_eq!(read(&process, LOWER_RESULT), 42.5_f64.to_bits());
        }
    }
}

pub fn invalid_operands_are_atomic_and_retryable() {
    for (numerator, divisor) in [(7.0_f64, 0.0_f64), (f64::NAN, 2.0)] {
        let mut process = Process32::load(&executable(), 40).unwrap();
        for (address, value) in [
            (LOWER, 42.5_f64),
            (NUMERATOR, numerator),
            (DIVISOR, divisor),
        ] {
            process
                .memory
                .write(u64::from(address), &value.to_le_bytes())
                .unwrap();
        }
        assert_eq!(process.run(4).instructions, 4);
        assert_eq!(process.cpu.eip, API);
        let before = process.cpu;
        let run = process.run(1);
        assert_eq!(run.reason, ProcessStop::UnsupportedApi { address: API });
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
    }

    let mut process = Process32::load(&executable(), 40).unwrap();
    for (address, value) in [(LOWER, 42.5_f64), (NUMERATOR, 7.0), (DIVISOR, 2.0)] {
        process
            .memory
            .write(u64::from(address), &value.to_le_bytes())
            .unwrap();
    }
    assert_eq!(process.run(4).instructions, 4);
    process.cpu.set_x87_control_word(0x027e);
    let before = process.cpu;
    assert_eq!(
        process.run(1).reason,
        ProcessStop::UnsupportedApi { address: API }
    );
    assert_eq!(process.cpu, before);
    process.cpu.set_x87_control_word(0x027f);
    assert_eq!(process.run(1).api_calls, 1);
    assert_eq!(process.cpu.eip, CODE + 24);
    assert_eq!(
        process.run(10).reason,
        ProcessStop::Stopped(StopReason::Breakpoint)
    );
    assert_eq!(read(&process, RESULT), 1.0_f64.to_bits());
    assert_eq!(read(&process, LOWER_RESULT), 42.5_f64.to_bits());
}

pub fn missing_second_operand_is_atomic() {
    let mut process = Process32::load(&executable(), 40).unwrap();
    process
        .memory
        .write(u64::from(DIVISOR), &2.0_f64.to_le_bytes())
        .unwrap();
    process.cpu.eip = CODE + 12;
    assert_eq!(process.run(2).instructions, 2);
    assert_eq!(process.cpu.eip, API);
    let before = process.cpu;
    assert_eq!(
        process.run(1).reason,
        ProcessStop::UnsupportedApi { address: API }
    );
    assert_eq!(process.cpu, before);
}
