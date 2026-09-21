#[path = "support/executable.rs"]
mod executable;
#[path = "support/x87_status_executable.rs"]
mod x87_status_executable;

use ring3_core::execution::{Cpu32, GuestMemory, Permissions, Register32, StopReason, load_pe32};

const LEFT: u32 = 0x0040_2200;
const RIGHT: u32 = LEFT + 8;

#[test]
fn comparison_and_sticky_precision_execute_whole_or_stepwise() {
    for budget in [1, 40] {
        let image = load_pe32(&x87_status_executable::pe32(), 32).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        let mut memory = image.memory;
        let mut count = 0;
        loop {
            let run = cpu.run(&mut memory, budget);
            count += run.instructions;
            if run.reason != StopReason::InstructionLimit {
                assert_eq!(run.reason, StopReason::Breakpoint);
                break;
            }
            assert!(count < 20);
        }
        assert_eq!(count, 8);
        assert_eq!(cpu.register(Register32::Esi), 0x3a20);
        assert_eq!(cpu.register(Register32::Eax), 0x20);
    }
}

fn instruction(opcode: u8, mode: u8, address: u32) -> Vec<u8> {
    let mut code = vec![opcode, mode];
    code.extend_from_slice(&address.to_le_bytes());
    code
}

#[test]
fn comparisons_ignore_precision_rounding_and_keep_integer_state() {
    for pc in [0, 0x100, 0x200, 0x300] {
        for rc in [0, 0x400, 0x800, 0xc00] {
            let mut code = instruction(0xdd, 0x05, LEFT);
            code.extend(instruction(0xd8, 0x15, RIGHT));
            code.extend([0xdf, 0xe0]);
            code.extend(instruction(0xd8, 0x1d, RIGHT));
            code.extend(instruction(0xdd, 0x3d, RIGHT + 8));
            let (mut cpu, mut memory) = load(&code, -3., 0.);
            memory
                .write(u64::from(RIGHT), &(-2_f32).to_le_bytes())
                .unwrap();
            let control = pc | rc | 0x7f;
            cpu.set_x87_control_word(control);
            assert_eq!(cpu.run(&mut memory, 3).instructions, 3);
            assert_eq!(cpu.register(Register32::Eax), 0xabcd_3900);
            memory
                .write(u64::from(RIGHT), &(-3_f32).to_le_bytes())
                .unwrap();
            assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
            let mut status = [0; 2];
            memory.read(u64::from(RIGHT + 8), &mut status).unwrap();
            assert_eq!(u16::from_le_bytes(status), 0x4000);
            assert_eq!(cpu.eflags, 0xced7);
            assert_eq!(cpu.x87_control_word(), control);
        }
    }
}

#[test]
fn arithmetic_precision_and_roundup_match_exact_rational_oracles() {
    for (operation, left, right, expected) in [
        (0, 3., 1., 0x20),
        (0, 10., 1., 0x220),
        (0, -10., 1., 0x220),
        (0, 2., 1., 0),
        (0, f64::MIN_POSITIVE, f64::MIN_POSITIVE, 0),
        (0, f64::MAX, f64::MAX, 0),
        (1, 2., 0., 0x220),
        (1, 3., 0., 0x20),
        (1, 4., 0., 0),
        (1, -0., 0., 0),
        (1, f64::MIN_POSITIVE, 0., 0),
        (2, 1.1, 1.1, 0x20),
        (2, 1.25, 1.5, 0),
        (2, 0., f64::MAX, 0),
        (2, f64::MAX, 0.5, 0),
        (2, f64::MIN_POSITIVE, 2., 0),
    ] {
        let mut code = instruction(0xdd, 0x05, LEFT);
        code.extend(match operation {
            0 => instruction(0xdc, 0x3d, RIGHT),
            1 => vec![0xd9, 0xfa],
            _ => instruction(0xdc, 0x0d, RIGHT),
        });
        code.extend([0xdf, 0xe0]);
        let (mut cpu, mut memory) = load(&code, left, right);
        assert_eq!(cpu.run(&mut memory, 3).instructions, 3);
        assert_eq!(
            cpu.register(Register32::Eax),
            0xabcd_3800 | expected,
            "{operation}/{left}/{right}"
        );
        assert_eq!(cpu.eflags, 0xced7);
    }
}

#[test]
fn stores_report_magnitude_rounding_and_precision_sticks_across_loads_and_comparisons() {
    for (value, expected) in [
        (0x3ff0_0000_1000_0000_u64, 0x20),
        (0x3ff0_0000_3000_0000, 0x220),
        (0xbff0_0000_1000_0000, 0x20),
        (0xbff0_0000_3000_0000, 0x220),
        (1_f64.to_bits(), 0),
    ] {
        let mut code = instruction(0xdd, 0x05, LEFT);
        code.extend(instruction(0xd9, 0x1d, RIGHT));
        code.extend([0xdf, 0xe0]);
        code.extend(instruction(0xdd, 0x05, LEFT));
        code.extend(instruction(0xdc, 0x1d, LEFT));
        code.extend([0xdf, 0xe0]);
        let (mut cpu, mut memory) = load(&code, f64::from_bits(value), 0.);
        assert_eq!(cpu.run(&mut memory, 3).instructions, 3);
        assert_eq!(cpu.register(Register32::Eax), 0xabcd_0000 | expected);
        assert_eq!(cpu.run(&mut memory, 3).instructions, 3);
        assert_eq!(
            cpu.register(Register32::Eax),
            0xabcd_4000 | (expected & 0x20)
        );
    }
    for (control, value, expected) in [
        (0x027f, -1.5, 0x220),
        (0x067f, -1.25, 0x220),
        (0x067f, 1.25, 0x20),
        (0x0a7f, 1.25, 0x220),
        (0x0e7f, -1.75, 0x20),
        (0x027f, 2., 0),
    ] {
        let mut code = instruction(0xdd, 0x05, LEFT);
        code.extend(instruction(0xdb, 0x1d, RIGHT));
        code.extend([0xdf, 0xe0]);
        let (mut cpu, mut memory) = load(&code, value, 0.);
        cpu.set_x87_control_word(control);
        assert_eq!(cpu.run(&mut memory, 3).instructions, 3);
        assert_eq!(cpu.register(Register32::Eax), 0xabcd_0000 | expected);
    }
}

#[test]
fn unsupported_comparisons_and_control_preserve_complete_cpu() {
    let mut code = instruction(0xdd, 0x05, LEFT);
    code.extend(instruction(0xdc, 0x1d, RIGHT));
    code.extend([0xdf, 0xe0]);
    for value in [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::from_bits(1),
    ] {
        let (mut cpu, mut memory) = load(&code, 1., value);
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        let before = cpu;
        assert_eq!(
            cpu.run(&mut memory, 1).reason,
            StopReason::UnsupportedInstruction
        );
        assert_eq!(cpu, before);
        memory
            .write(u64::from(RIGHT), &1_f64.to_le_bytes())
            .unwrap();
        assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
        assert_eq!(cpu.register(Register32::Eax), 0xabcd_4000);
    }
    for offset in [6, 12] {
        let (mut cpu, mut memory) = load(&code, 1., 1.);
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        cpu.eip = 0x0040_1000 + offset;
        cpu.set_x87_control_word(0x025f);
        let before = cpu;
        assert_eq!(
            cpu.run(&mut memory, 1).reason,
            StopReason::UnsupportedInstruction
        );
        assert_eq!(cpu, before);
    }
    let (mut cpu, mut memory) = load(&code, 1., 1.);
    cpu.eip += 6;
    let before = cpu;
    assert_eq!(
        cpu.run(&mut memory, 1).reason,
        StopReason::UnsupportedInstruction
    );
    assert_eq!(cpu, before);
}

#[test]
fn comparison_faults_and_status_write_faults_are_atomic_and_retryable() {
    for opcode in [0xd8, 0xdc] {
        for address in [0x6000_0000, u32::MAX] {
            let mut code = instruction(0xdd, 0x05, LEFT);
            code.extend(instruction(opcode, 0x1d, address));
            let (mut cpu, mut memory) = load(&code, 1., 1.);
            assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
            let before = cpu;
            assert!(matches!(
                cpu.run(&mut memory, 1).reason,
                StopReason::MemoryFault(_)
            ));
            assert_eq!(cpu, before);
        }
    }
    let mut code = instruction(0xdd, 0x05, LEFT);
    code.extend(instruction(0xdc, 0x1d, RIGHT));
    code.extend(instruction(0xdd, 0x3d, 0x6000_0fff));
    let (mut cpu, mut memory) = load(&code, 1., 2.);
    memory
        .map_zeroed(0x6000_0000, 8192, Permissions::READ_WRITE)
        .unwrap();
    memory.write(0x6000_0fff, &[0xaa, 0xbb]).unwrap();
    memory
        .protect(0x6000_1000, 4096, Permissions::READ)
        .unwrap();
    assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
    let before = cpu;
    assert_eq!(cpu.run(&mut memory, 0).instructions, 0);
    assert_eq!(cpu, before);
    assert!(matches!(
        cpu.run(&mut memory, 1).reason,
        StopReason::MemoryFault(_)
    ));
    assert_eq!(cpu, before);
    let mut bytes = [0; 2];
    memory.read(0x6000_0fff, &mut bytes).unwrap();
    assert_eq!(bytes, [0xaa, 0xbb]);
    memory
        .protect(0x6000_1000, 4096, Permissions::READ_WRITE)
        .unwrap();
    assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
    memory.read(0x6000_0fff, &mut bytes).unwrap();
    assert_eq!(bytes, [0, 1]);
}

#[test]
fn failed_rounded_stores_preserve_precision_and_fs_status_stores_wrap_the_base() {
    let mut code = instruction(0xdd, 0x05, LEFT);
    code.extend(instruction(0xd9, 0x1d, 0x6000_0000));
    code.extend([0xdf, 0xe0]);
    let (mut cpu, mut memory) = load(&code, 1.000_000_1, 0.);
    assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
    let before = cpu;
    assert!(matches!(
        cpu.run(&mut memory, 1).reason,
        StopReason::MemoryFault(_)
    ));
    assert_eq!(cpu, before);
    cpu.eip += 6;
    assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
    assert_eq!(cpu.register(Register32::Eax), 0xabcd_3800);
    let mut code = vec![0x64];
    code.extend(instruction(0xdd, 0x3d, 0xffff_ffe0));
    let (mut cpu, mut memory) = load(&code, 0., 0.);
    cpu.set_fs_base(0x0040_2220);
    memory.write(u64::from(LEFT), &[0xaa, 0xbb, 0xcc]).unwrap();
    assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
    let mut bytes = [0; 3];
    memory.read(u64::from(LEFT), &mut bytes).unwrap();
    assert_eq!(bytes, [0, 0, 0xcc]);
}

#[test]
fn fs_comparison_and_last_status_word_obey_guest_address_bounds() {
    let mut code = instruction(0xdd, 0x05, LEFT);
    code.push(0x64);
    code.extend(instruction(0xd8, 0x1d, 0xffff_fff0));
    code.extend([0xdf, 0xe0]);
    let (mut cpu, mut memory) = load(&code, 1., 0.);
    cpu.set_fs_base(RIGHT + 16);
    memory
        .write(u64::from(RIGHT), &2_f32.to_le_bytes())
        .unwrap();
    assert_eq!(cpu.run(&mut memory, 3).instructions, 3);
    assert_eq!(cpu.register(Register32::Eax), 0xabcd_0100);
    for address in [u32::MAX - 1, u32::MAX] {
        let code = instruction(0xdd, 0x3d, address);
        let (mut cpu, mut memory) = load(&code, 0., 0.);
        memory
            .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
            .unwrap();
        memory.write(0xffff_fffd, &[0xcc, 0xaa, 0xbb]).unwrap();
        let before = cpu;
        let run = cpu.run(&mut memory, 1);
        let mut bytes = [0; 3];
        memory.read(0xffff_fffd, &mut bytes).unwrap();
        if address == u32::MAX {
            assert!(matches!(run.reason, StopReason::MemoryFault(_)));
            assert_eq!(cpu, before);
            assert_eq!(bytes, [0xcc, 0xaa, 0xbb]);
        } else {
            assert_eq!(run.instructions, 1);
            assert_eq!(bytes, [0xcc, 0, 0]);
        }
    }
}

fn load(code: &[u8], left: f64, right: f64) -> (Cpu32, GuestMemory) {
    let image = load_pe32(&executable::pe32(code), 64).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_x87_control_word(0x027f);
    cpu.set_register(Register32::Eax, 0xabcd_1234);
    cpu.eflags = 0xced7;
    let mut memory = image.memory;
    memory.write(u64::from(LEFT), &left.to_le_bytes()).unwrap();
    memory
        .write(u64::from(RIGHT), &right.to_le_bytes())
        .unwrap();
    (cpu, memory)
}

#[test]
fn comparisons_expose_condition_codes_and_stack_top() {
    for (left, right, condition) in [
        (1., 2., 0x100),
        (2., 1., 0),
        (1., 1., 0x4000),
        (-0., 0., 0x4000),
    ] {
        for pop in [false, true] {
            let mut code = instruction(0xdd, 0x05, LEFT);
            code.extend(instruction(0xdc, if pop { 0x1d } else { 0x15 }, RIGHT));
            code.extend([0xdf, 0xe0]);
            let (mut cpu, mut memory) = load(&code, left, right);
            assert_eq!(cpu.run(&mut memory, 3).instructions, 3);
            assert_eq!(
                cpu.register(Register32::Eax),
                0xabcd_0000 | condition | if pop { 0 } else { 0x3800 }
            );
            assert_eq!(cpu.eflags, 0xced7);
            assert_eq!(cpu.x87_control_word(), 0x027f);
        }
    }
}
