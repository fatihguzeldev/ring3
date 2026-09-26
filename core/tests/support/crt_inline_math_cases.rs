use super::imported_executable;
use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

const CODE: u32 = 0x0040_1000;
const LOWER: u32 = 0x0040_2180;
const FIRST: u32 = LOWER + 8;
const SECOND: u32 = LOWER + 16;
const RESULT: u32 = LOWER + 24;
const LOWER_RESULT: u32 = LOWER + 32;

fn executable(symbol: &str, binary: bool) -> Vec<u8> {
    let mut code = Vec::new();
    for address in [LOWER, FIRST] {
        code.extend([0xdd, 0x05]);
        code.extend(address.to_le_bytes());
    }
    if binary {
        code.extend([0xdd, 0x05]);
        code.extend(SECOND.to_le_bytes());
    }
    code.extend([0xff, 0x15, 0x60, 0x20, 0x40, 0]);
    for address in [RESULT, LOWER_RESULT] {
        code.extend([0xdd, 0x1d]);
        code.extend(address.to_le_bytes());
    }
    code.push(0xcc);
    imported_executable::pe32(&code, "MSVCRT.dll", &[symbol])
}

fn read(process: &Process32, address: u32) -> u64 {
    let mut bytes = [0; 8];
    process.memory.read(u64::from(address), &mut bytes).unwrap();
    u64::from_le_bytes(bytes)
}

fn prepare(symbol: &str, first: f64, second: Option<f64>) -> Process32 {
    let mut process = Process32::load(&executable(symbol, second.is_some()), 40).unwrap();
    for (address, value) in [
        (LOWER, Some(42.5_f64)),
        (FIRST, Some(first)),
        (SECOND, second),
    ] {
        if let Some(value) = value {
            process
                .memory
                .write(u64::from(address), &value.to_le_bytes())
                .unwrap();
        }
    }
    process
}

fn assert_value(actual_bits: u64, expected: f64) {
    let actual = f64::from_bits(actual_bits);
    if expected == 0.0 {
        assert_eq!(actual_bits, expected.to_bits());
    } else {
        assert!(actual.is_finite());
        assert!(
            (actual - expected).abs() <= 2.0e-15,
            "{actual} != {expected}"
        );
    }
}

pub fn finite_values_across_budgets() {
    let cases = [
        ("_CIasin", 0.0_f64, None, 0.0),
        ("_CIasin", -0.0, None, -0.0),
        ("_CIasin", 1.0, None, std::f64::consts::FRAC_PI_2),
        ("_CIasin", -1.0, None, -std::f64::consts::FRAC_PI_2),
        ("_CIasin", 0.5, None, std::f64::consts::FRAC_PI_6),
        ("_CIacos", 1.0, None, 0.0),
        ("_CIacos", -1.0, None, std::f64::consts::PI),
        ("_CIacos", 0.0, None, std::f64::consts::FRAC_PI_2),
        ("_CIacos", 0.5, None, std::f64::consts::FRAC_PI_3),
        ("_CIpow", 2.0, Some(3.0), 8.0),
        ("_CIpow", 3.0, Some(2.0), 9.0),
        ("_CIpow", 4.0, Some(0.5), 2.0),
        ("_CIpow", -2.0, Some(3.0), -8.0),
        ("_CIpow", -2.0, Some(2.0), 4.0),
        ("_CIpow", 2.0, Some(-3.0), 0.125),
        ("_CIpow", 0.0, Some(0.0), 1.0),
    ];
    for (symbol, first, second, expected) in cases {
        for budget in [1, 3, 30] {
            let mut process = prepare(symbol, first, second);
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
            assert_eq!(counts, (if second.is_some() { 7 } else { 6 }, 1));
            assert_eq!(process.cpu.register(Register32::Esp), stack);
            assert_eq!(process.cpu.register(Register32::Eax), 0x1234_5678);
            assert_eq!(process.cpu.eflags, 0x246);
            assert_eq!(process.cpu.x87_control_word(), 0x027f);
            assert_value(read(&process, RESULT), expected);
            assert_eq!(read(&process, LOWER_RESULT), 42.5_f64.to_bits());
        }
    }
}

pub fn invalid_domains_are_atomic() {
    for (symbol, first, second) in [
        ("_CIasin", 1.5_f64, None),
        ("_CIacos", -1.5, None),
        ("_CIasin", f64::NAN, None),
        ("_CIpow", -2.0, Some(0.5)),
        ("_CIpow", 0.0, Some(-1.0)),
        ("_CIpow", 1.0e200, Some(2.0)),
        ("_CIpow", 1.0e-200, Some(2.0)),
    ] {
        let mut process = prepare(symbol, first, second);
        let steps = if second.is_some() { 4 } else { 3 };
        assert_eq!(process.run(steps).instructions, steps);
        let before = process.cpu;
        let address = process.cpu.eip;
        let run = process.run(1);
        assert_eq!(run.reason, ProcessStop::UnsupportedApi { address });
        assert_eq!((run.instructions, run.api_calls), (0, 0));
        assert_eq!(process.cpu, before);
    }
}

pub fn missing_operand_and_unmasked_control_are_retryable() {
    for (symbol, binary) in [("_CIasin", false), ("_CIacos", false), ("_CIpow", true)] {
        let mut process = prepare(symbol, 0.5, binary.then_some(2.0));
        process.cpu.eip = CODE + if binary { 18 } else { 12 };
        assert_eq!(process.run(1).instructions, 1);
        let before = process.cpu;
        let address = before.eip;
        assert_eq!(
            process.run(1).reason,
            ProcessStop::UnsupportedApi { address }
        );
        assert_eq!(process.cpu, before);

        let mut process = prepare(symbol, 0.5, binary.then_some(2.0));
        assert_eq!(
            process.run(if binary { 4 } else { 3 }).reason,
            ProcessStop::Stopped(StopReason::InstructionLimit)
        );
        process.cpu.set_x87_control_word(0x027e);
        let before = process.cpu;
        assert_eq!(
            process.run(1).reason,
            ProcessStop::UnsupportedApi {
                address: before.eip
            }
        );
        assert_eq!(process.cpu, before);
        process.cpu.set_x87_control_word(0x027f);
        assert_eq!(process.run(1).api_calls, 1);
        assert_eq!(
            process.run(10).reason,
            ProcessStop::Stopped(StopReason::Breakpoint)
        );
        assert_eq!(read(&process, LOWER_RESULT), 42.5_f64.to_bits());
    }
}
