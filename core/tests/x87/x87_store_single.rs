use super::executable;

use ring3_core::execution::{Cpu32, GuestMemory, Register32, StopReason, load_pe32};

const INPUT: u32 = 0x0040_2200;
const COPY: u32 = 0x0040_2220;
const POPPED: u32 = 0x0040_2228;

fn load(code: &[u8], value: f64, control: u16) -> (Cpu32, GuestMemory) {
    let mut image = load_pe32(&executable::pe32(code), 16).unwrap();
    image
        .memory
        .write(u64::from(INPUT), &value.to_le_bytes())
        .unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_x87_control_word(control);
    (cpu, image.memory)
}

fn read(memory: &GuestMemory, address: u32) -> u64 {
    let mut bytes = [0; 8];
    memory.read(u64::from(address), &mut bytes).unwrap();
    u64::from_le_bytes(bytes)
}

#[test]
fn single_precision_control_stores_binary64_bits_without_implicit_narrowing() {
    let code = [
        0xdd, 0x05, 0x00, 0x22, 0x40, 0x00, 0xdd, 0x15, 0x20, 0x22, 0x40, 0x00, 0xdf, 0xe0, 0xdd,
        0x1d, 0x28, 0x22, 0x40, 0x00, 0xdf, 0xe0,
    ];
    for value in [0.0, -0.0, 1.0 / 3.0] {
        let (mut cpu, mut memory) = load(&code, value, 0x007f);
        assert_eq!(cpu.run(&mut memory, 3).instructions, 3);
        assert_eq!(cpu.register(Register32::Eax) & 0x3800, 0x3800);
        assert_eq!(read(&memory, COPY), value.to_bits());
        assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
        assert_eq!(read(&memory, POPPED), value.to_bits());
        assert_eq!(cpu.register(Register32::Eax) & 0x3800, 0);
    }
}

#[test]
fn single_precision_binary64_store_rejects_empty_stack_bad_control_and_fault_atomically() {
    let store = [0xdd, 0x15, 0x20, 0x22, 0x40, 0x00];
    let (mut cpu, mut memory) = load(&store, 1.0, 0x007f);
    let before = cpu;
    assert_eq!(
        cpu.run(&mut memory, 1).reason,
        StopReason::UnsupportedInstruction
    );
    assert_eq!(cpu, before);

    for (opcode, control) in [(0x15, 0x0a7f), (0x1d, 0x0a7f)] {
        let code = [
            0xdd, 0x05, 0x00, 0x22, 0x40, 0x00, 0xdd, opcode, 0x20, 0x22, 0x40, 0x00,
        ];
        let (mut cpu, mut memory) = load(&code, 1.0, control);
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        let before = cpu;
        assert_eq!(
            cpu.run(&mut memory, 1).reason,
            StopReason::UnsupportedInstruction
        );
        assert_eq!(cpu, before);
        assert_eq!(read(&memory, COPY), 0);
    }
    let code = [
        0xdd, 0x05, 0x00, 0x22, 0x40, 0x00, 0xdd, 0x1d, 0x00, 0x00, 0x00, 0x60,
    ];
    let (mut cpu, mut memory) = load(&code, 1.0, 0x007f);
    assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
    let before = cpu;
    assert!(matches!(
        cpu.run(&mut memory, 1).reason,
        StopReason::MemoryFault(_)
    ));
    assert_eq!(cpu, before);
}
