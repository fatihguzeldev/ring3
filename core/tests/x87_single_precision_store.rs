#[path = "support/executable.rs"]
mod executable;

use ring3_core::execution::{Cpu32, GuestMemory, Register32, StopReason, load_pe32};

const TOP: u32 = 0x0040_2200;
const OUTPUT: u32 = TOP + 0x20;

fn load(value: f64, destination: u32) -> (Cpu32, GuestMemory) {
    let mut code = vec![0xdd, 0x05];
    code.extend(TOP.to_le_bytes());
    code.extend([0xd9, 0x1d]);
    code.extend(destination.to_le_bytes());
    code.extend([0xdf, 0xe0, 0xcc]);
    let mut image = load_pe32(&executable::pe32(&code), 16).unwrap();
    image
        .memory
        .write(u64::from(TOP), &value.to_le_bytes())
        .unwrap();
    image.memory.write(u64::from(OUTPUT), &[0x55; 4]).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_x87_control_word(0x007f);
    cpu.eflags = 0xced7;
    (cpu, image.memory)
}

fn output(memory: &GuestMemory) -> u32 {
    let mut bytes = [0; 4];
    memory.read(u64::from(OUTPUT), &mut bytes).unwrap();
    u32::from_le_bytes(bytes)
}

#[test]
fn store_rounds_to_m32_sets_status_and_pops() {
    for (input, expected, status) in [
        (1.5_f64, 1.5_f32.to_bits(), 0),
        (-0.0, (-0.0_f32).to_bits(), 0),
        (1.0 + 2.0_f64.powi(-24), 1.0_f32.to_bits(), 0x20),
        (
            1.0 + 3.0 * 2.0_f64.powi(-24),
            (1.0_f32 + 2.0_f32.powi(-22)).to_bits(),
            0x220,
        ),
    ] {
        let (mut cpu, mut memory) = load(input, OUTPUT);
        assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
        assert_eq!(output(&memory), expected);
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        assert_eq!(cpu.register(Register32::Eax) & 0x220, status);
        assert_eq!(cpu.eflags, 0xced7);
        let before = cpu;
        cpu.eip -= 8;
        let retry = cpu;
        assert_eq!(
            cpu.run(&mut memory, 1).reason,
            StopReason::UnsupportedInstruction
        );
        assert_eq!(cpu, retry);
        assert_ne!(cpu, before);
    }
}

#[test]
fn fault_range_control_and_empty_stack_preserve_cpu_and_output() {
    for case in 0..5 {
        let (value, destination) = match case {
            0 => (f64::MAX, OUTPUT),
            1 => (f64::from(f32::MIN_POSITIVE).next_down(), OUTPUT),
            2 => (1.5, 0x5000_0000),
            _ => (1.5, OUTPUT),
        };
        let (mut cpu, mut memory) = load(value, destination);
        if case == 4 {
            cpu.eip += 6;
        } else {
            assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        }
        if case == 3 {
            cpu.set_x87_control_word(0x037f);
        }
        let before = cpu;
        let stop = cpu.run(&mut memory, 1).reason;
        if case == 2 {
            assert!(matches!(stop, StopReason::MemoryFault(_)));
        } else {
            assert_eq!(stop, StopReason::UnsupportedInstruction);
        }
        assert_eq!(cpu, before);
        assert_eq!(output(&memory), 0x5555_5555);
    }
}
