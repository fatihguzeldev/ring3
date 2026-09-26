use super::executable;

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
            1 => (f64::from(f32::MAX).next_up(), OUTPUT),
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

#[test]
fn nonpop_m32_store_honors_nearest_and_truncate_control() {
    for (value, control, expected, status) in [
        (0.5_f64, 0x0c7f_u16, 0.5_f32.to_bits(), 0),
        (-0.0_f64, 0x0c7f, (-0.0_f32).to_bits(), 0),
        (
            1.0 + 3.0 * 2.0_f64.powi(-24),
            0x0c7f,
            (1.0_f32 + 2.0_f32.powi(-23)).to_bits(),
            0x20,
        ),
        (
            -1.0 - 3.0 * 2.0_f64.powi(-24),
            0x0c7f,
            (-1.0_f32 - 2.0_f32.powi(-23)).to_bits(),
            0x20,
        ),
        (
            1.0 + 3.0 * 2.0_f64.powi(-24),
            0x007f,
            (1.0_f32 + 2.0_f32.powi(-22)).to_bits(),
            0x220,
        ),
    ] {
        let mut code = vec![0xdd, 0x05];
        code.extend(TOP.to_le_bytes());
        code.extend([0xd9, 0x15]);
        code.extend(OUTPUT.to_le_bytes());
        code.extend([0xdf, 0xe0, 0xdd, 0x1d]);
        code.extend((OUTPUT + 8).to_le_bytes());
        code.push(0xcc);
        let mut image = load_pe32(&executable::pe32(&code), 16).unwrap();
        image
            .memory
            .write(u64::from(TOP), &value.to_le_bytes())
            .unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        cpu.set_x87_control_word(control);
        assert_eq!(cpu.run(&mut image.memory, 3).instructions, 3);
        assert_eq!(output(&image.memory), expected);
        assert_eq!(cpu.register(Register32::Eax) & 0x220, status);
        cpu.set_x87_control_word(0x027f);
        assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
        assert_eq!(output(&image.memory), expected);
        let mut original = [0; 8];
        image
            .memory
            .read(u64::from(OUTPUT + 8), &mut original)
            .unwrap();
        assert_eq!(u64::from_le_bytes(original), value.to_bits());
    }
}

#[test]
fn frame_relative_fst_m32_matches_game_control_profile() {
    let mut code = vec![0xdd, 0x05];
    code.extend(TOP.to_le_bytes());
    code.extend([0xd9, 0x55, 0x08, 0xcc]);
    let mut image = load_pe32(&executable::pe32(&code), 16).unwrap();
    image
        .memory
        .write(u64::from(TOP), &0.5_f64.to_le_bytes())
        .unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_register(Register32::Ebp, OUTPUT - 8);
    cpu.set_x87_control_word(0x0c7f);
    assert_eq!(cpu.run(&mut image.memory, 2).instructions, 2);
    assert_eq!(output(&image.memory), 0.5_f32.to_bits());
}

#[test]
fn truncating_m32_pop_store_keeps_faults_atomic() {
    let (mut cpu, mut memory) = load(1.0 + 3.0 * 2.0_f64.powi(-24), OUTPUT);
    cpu.set_x87_control_word(0x0c7f);
    assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
    let before = cpu;
    assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
    assert_eq!(output(&memory), (1.0_f32 + 2.0_f32.powi(-23)).to_bits());
    assert_ne!(cpu, before);
    cpu.eip -= 6;
    let after = cpu;
    assert_eq!(
        cpu.run(&mut memory, 1).reason,
        StopReason::UnsupportedInstruction
    );
    assert_eq!(cpu, after);

    let (mut cpu, mut memory) = load(0.5, 0x5000_0000);
    cpu.set_x87_control_word(0x0c7f);
    assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
    let before = cpu;
    assert!(matches!(
        cpu.run(&mut memory, 1).reason,
        StopReason::MemoryFault(_)
    ));
    assert_eq!(cpu, before);

    for (value, control) in [
        (f64::MAX, 0x0c7f),
        (f64::from(f32::MAX).next_up(), 0x0c7f),
        (0.5, 0x087f),
    ] {
        let (mut cpu, mut memory) = load(value, OUTPUT);
        cpu.set_x87_control_word(control);
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        let before = cpu;
        assert_eq!(
            cpu.run(&mut memory, 1).reason,
            StopReason::UnsupportedInstruction
        );
        assert_eq!(cpu, before);
        assert_eq!(output(&memory), 0x5555_5555);
    }
}
