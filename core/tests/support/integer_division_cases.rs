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
