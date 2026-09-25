use ring3_core::execution::{Cpu32, GuestMemory, Register32, StopReason, load_pe32};

pub const TOP: u32 = 0x0040_2200;
pub const SOURCE: u32 = TOP + 0x10;
pub const RESULT: u32 = TOP + 0x20;

pub fn load(top: f64, source: i32, address: u32, fs: bool) -> (Cpu32, GuestMemory) {
    let mut code = vec![0xdd, 0x05];
    code.extend(TOP.to_le_bytes());
    if fs {
        code.push(0x64);
    }
    code.extend([0xda, 0x35]);
    code.extend(address.to_le_bytes());
    code.extend([0xdf, 0xe0, 0xdd, 0x1d]);
    code.extend(RESULT.to_le_bytes());
    code.push(0xcc);
    let mut image = load_pe32(&super::executable::pe32(&code), 16).unwrap();
    image
        .memory
        .write(u64::from(TOP), &top.to_le_bytes())
        .unwrap();
    image
        .memory
        .write(u64::from(SOURCE), &source.to_le_bytes())
        .unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_x87_control_word(0x027f);
    cpu.set_register(Register32::Eax, 0xabcd_1234);
    cpu.eflags = 0xced7;
    (cpu, image.memory)
}

pub fn result(memory: &GuestMemory) -> u64 {
    let mut bytes = [0; 8];
    memory.read(u64::from(RESULT), &mut bytes).unwrap();
    u64::from_le_bytes(bytes)
}

pub fn signed_quotients_across_budgets() {
    // inexact expectations come from exact rational quotients, not host division.
    for (top, source, expected, status) in [
        (8.0, 2, 0x4010_0000_0000_0000, 0),
        (-8.0, -2, 0x4010_0000_0000_0000, 0),
        (0.0, -7, 0x8000_0000_0000_0000, 0),
        (-0.0, -7, 0, 0),
        (32.0, 13, 0x4003_b13b_13b1_3b14, 0x220),
        (-32.0, 13, 0xc003_b13b_13b1_3b14, 0x220),
        (1.0, 3, 0x3fd5_5555_5555_5555, 0x20),
        (-1.0, 3, 0xbfd5_5555_5555_5555, 0x20),
        (1.0, -3, 0xbfd5_5555_5555_5555, 0x20),
        (-1.0, -3, 0x3fd5_5555_5555_5555, 0x20),
        (1.0, i32::MAX, 0x3e00_0000_0020_0000, 0x20),
        (1.0, i32::MIN, 0xbe00_0000_0000_0000, 0),
        (f64::from(i32::MAX), i32::MAX, 0x3ff0_0000_0000_0000, 0),
    ] {
        for budget in [1, 2, 20] {
            let (mut cpu, mut memory) = load(top, source, SOURCE, false);
            let before = cpu;
            assert_eq!(cpu.run(&mut memory, 0).instructions, 0);
            assert_eq!(cpu, before);
            let mut steps = 0;
            loop {
                let run = cpu.run(&mut memory, budget);
                steps += run.instructions;
                if run.reason == StopReason::Breakpoint {
                    break;
                }
                assert_eq!(run.reason, StopReason::InstructionLimit);
                assert!(steps < 5);
            }
            assert_eq!(steps, 5);
            assert_eq!(result(&memory), expected);
            assert_eq!(cpu.register(Register32::Eax), 0xabcd_3800 | status);
            assert_eq!(cpu.eflags, 0xced7);
            assert_eq!(cpu.x87_control_word(), 0x027f);
            let mut bytes = [0; 4];
            memory.read(u64::from(SOURCE), &mut bytes).unwrap();
            assert_eq!(i32::from_le_bytes(bytes), source);
        }
    }
}

pub fn single_quotients_across_budgets() {
    // fixed bits use exact rational rounding; the final two cases defeat double rounding.
    for (top, source, expected, status) in [
        (0x40d0_0000_0000_0000, 32, 0x4080_0000_0000_0000, 0),
        (0, 7, 0, 0),
        (0, -7, 0x8000_0000_0000_0000, 0),
        (0x3ff0_0000_0000_0000, 3, 0x3fd5_5555_6000_0000, 0x220),
        (0x3ff0_0000_0000_0000, -3, 0xbfd5_5555_6000_0000, 0x220),
        (0x4040_0000_0000_0000, 13, 0x4003_b13b_2000_0000, 0x220),
        (0x3ff0_0000_0000_0000, i32::MAX, 0x3e00_0000_0000_0000, 0x20),
        (0x3ff0_0000_0000_0000, i32::MIN, 0xbe00_0000_0000_0000, 0),
        (0x41df_ffff_ffc0_0000, i32::MAX, 0x3ff0_0000_0000_0000, 0),
        (0x3ff0_0000_1000_0000, 1, 0x3ff0_0000_0000_0000, 0x20),
        (0x3ff0_0000_3000_0000, 1, 0x3ff0_0000_4000_0000, 0x220),
        (0x3810_0000_0000_0000, 1, 0x3810_0000_0000_0000, 0),
        (0x47ef_ffff_e000_0000, 1, 0x47ef_ffff_e000_0000, 0),
        (
            0x41d0_0000_10c0_0001,
            1_073_741_827,
            0x3ff0_0000_2000_0000,
            0x220,
        ),
        (
            0x41d0_0000_30c0_0002,
            1_073_741_827,
            0x3ff0_0000_2000_0000,
            0x20,
        ),
    ] {
        for sign in [0, 1 << 63] {
            for budget in [1, 2, 20] {
                let (mut cpu, mut memory) = load(f64::from_bits(top ^ sign), source, SOURCE, false);
                cpu.set_x87_control_word(0x007f);
                let before = cpu;
                assert_eq!(cpu.run(&mut memory, 0).instructions, 0);
                assert_eq!(cpu, before);
                let mut steps = 0;
                loop {
                    let run = cpu.run(&mut memory, budget);
                    steps += run.instructions;
                    if run.reason == StopReason::Breakpoint {
                        break;
                    }
                    assert_eq!(run.reason, StopReason::InstructionLimit);
                    assert!(steps < 5);
                }
                assert_eq!(steps, 5);
                assert_eq!(result(&memory), expected ^ sign);
                assert_eq!(cpu.register(Register32::Eax), 0xabcd_3800 | status);
                assert_eq!(cpu.eflags, 0xced7);
                assert_eq!(cpu.x87_control_word(), 0x007f);
                let mut bytes = [0; 4];
                memory.read(u64::from(SOURCE), &mut bytes).unwrap();
                assert_eq!(i32::from_le_bytes(bytes), source);
            }
        }
    }
}
