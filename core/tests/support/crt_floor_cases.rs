use ring3_core::execution::{Process32, ProcessStop, Register32, StopReason};

pub const RESULT: u32 = 0x0040_2180;

pub fn executable(input: u64) -> Vec<u8> {
    let mut code = vec![0x68];
    code.extend((input >> 32).to_le_bytes()[..4].iter().copied());
    code.push(0x68);
    code.extend(input.to_le_bytes()[..4].iter().copied());
    code.extend([0xff, 0x15, 0x60, 0x20, 0x40, 0, 0x83, 0xc4, 8, 0xdd, 0x1d]);
    code.extend(RESULT.to_le_bytes());
    code.push(0xcc);
    super::imported_executable::pe32(&code, "MSVCRT.dll", &["floor"])
}

pub fn finite_results_across_budgets() {
    for (input, expected) in [
        (0_u64, 0_u64),
        (0x8000_0000_0000_0000, 0x8000_0000_0000_0000),
        (0x4043_8000_0000_0000, 0x4043_8000_0000_0000),
        (0x3ffc_0000_0000_0000, 0x3ff0_0000_0000_0000),
        (0xbffc_0000_0000_0000, 0xc000_0000_0000_0000),
        (0x3fe0_0000_0000_0000, 0),
        (0xbfe0_0000_0000_0000, 0xbff0_0000_0000_0000),
        (1, 0),
        (0x8000_0000_0000_0001, 0xbff0_0000_0000_0000),
        (0x0010_0000_0000_0000, 0),
        (0x8010_0000_0000_0000, 0xbff0_0000_0000_0000),
        (0x433f_ffff_ffff_ffff, 0x433f_ffff_ffff_ffff),
        (0x7fef_ffff_ffff_ffff, 0x7fef_ffff_ffff_ffff),
        (0xffef_ffff_ffff_ffff, 0xffef_ffff_ffff_ffff),
    ] {
        for budget in [1, 3, 30] {
            let mut p = Process32::load(&executable(input), 32).unwrap();
            p.cpu.set_register(Register32::Eax, 0x1234_5678);
            let mut counts = (0, 0);
            loop {
                let run = p.run(budget);
                counts.0 += run.instructions;
                counts.1 += run.api_calls;
                if run.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                    assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                    break;
                }
                assert!(counts.0 + counts.1 < 30);
            }
            assert_eq!(counts, (6, 1));
            assert_eq!(p.cpu.register(Register32::Esp), 0x1001_0000);
            assert_eq!(p.cpu.register(Register32::Eax), 0x1234_5678);
            assert_eq!(p.cpu.x87_control_word(), 0x027f);
            let mut bytes = [0; 8];
            p.memory.read(u64::from(RESULT), &mut bytes).unwrap();
            assert_eq!(u64::from_le_bytes(bytes), expected);
        }
    }
}
