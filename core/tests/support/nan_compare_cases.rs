use ring3_core::execution::{Cpu32, GuestMemory, Register32, StopReason, load_pe32};

pub const INPUT: u32 = 0x0040_2200;
pub const SOURCE: u32 = INPUT + 8;
pub const OUTPUT: u32 = INPUT + 16;

pub fn load(bits: u64, opcode: u8, mode: u8, address: u32) -> (Cpu32, GuestMemory) {
    let mut code = vec![0xdd, 0x05];
    code.extend(INPUT.to_le_bytes());
    code.extend([opcode, mode]);
    code.extend(address.to_le_bytes());
    code.extend([0xdd, 0x1d]);
    code.extend(OUTPUT.to_le_bytes());
    code.extend([0xdf, 0xe0, 0xcc]);
    let mut image = load_pe32(&super::executable::pe32(&code), 16).unwrap();
    image
        .memory
        .write(u64::from(INPUT), &bits.to_le_bytes())
        .unwrap();
    if opcode == 0xd8 {
        image
            .memory
            .write(u64::from(SOURCE), &1_f32.to_le_bytes())
            .unwrap();
    } else {
        image
            .memory
            .write(u64::from(SOURCE), &1_f64.to_le_bytes())
            .unwrap();
    }
    image.memory.write(u64::from(OUTPUT), &[0x55; 8]).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_x87_control_word(0x027f);
    cpu.set_fs_base(0xdead_0000);
    cpu.set_register(Register32::Eax, 0xabcd_1234);
    cpu.eflags = 0xced7;
    (cpu, image.memory)
}

pub fn read(memory: &GuestMemory, address: u32) -> u64 {
    let mut bytes = [0; 8];
    memory.read(u64::from(address), &mut bytes).unwrap();
    u64::from_le_bytes(bytes)
}

pub fn nan_top_refuses_comparison_atomically() {
    for bits in [
        0x7ff8_0000_0000_0011,
        0xfff8_0000_0000_0011,
        0x7ff0_0000_0000_0001,
        0xfff0_0000_0000_0001,
    ] {
        for (opcode, mode) in [(0xd8, 0x15), (0xdc, 0x15), (0xd8, 0x1d), (0xdc, 0x1d)] {
            for control in [0x007f, 0x027f, 0x037f, 0x0c7f] {
                let (mut loaded, mut memory) = load(bits, opcode, mode, SOURCE);
                loaded.set_x87_control_word(control);
                assert_eq!(loaded.run(&mut memory, 1).instructions, 1);
                for budget in [1, 20] {
                    let (mut cpu, mut memory) = load(bits, opcode, mode, SOURCE);
                    cpu.set_x87_control_word(control);
                    if budget == 1 {
                        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
                    }
                    let before = cpu;
                    assert_eq!(cpu.run(&mut memory, 0).instructions, 0);
                    assert_eq!(cpu, before);
                    let source = read(&memory, SOURCE);
                    let run = cpu.run(&mut memory, budget);
                    assert_eq!(run.reason, StopReason::UnsupportedInstruction);
                    assert_eq!(run.instructions, u64::from(budget != 1));
                    assert_eq!(cpu, loaded);
                    assert_eq!(read(&memory, INPUT), bits);
                    assert_eq!(read(&memory, SOURCE), source);
                    assert_eq!(read(&memory, OUTPUT), 0x5555_5555_5555_5555);
                }
            }
        }
    }
}
