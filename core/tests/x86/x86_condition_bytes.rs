use super::condition_bytes_executable;
use super::executable;

use ring3_core::execution::{Cpu32, PAGE_SIZE, Permissions, Register32, StopReason, load_pe32};

#[test]
fn all_conditions_write_low_and_high_bytes_without_changing_other_cpu_state() {
    // truth bits enumerate cf,pf,zf,sf,of independently of the eflags layout.
    let truth = [
        0xffff_0000_u32,
        0x0000_ffff,
        0xaaaa_aaaa,
        0x5555_5555,
        0xf0f0_f0f0,
        0x0f0f_0f0f,
        0xfafa_fafa,
        0x0505_0505,
        0xff00_ff00,
        0x00ff_00ff,
        0xcccc_cccc,
        0x3333_3333,
        0x00ff_ff00,
        0xff00_00ff,
        0xf0ff_fff0,
        0x0f00_000f,
    ];
    let parents = [
        Register32::Eax,
        Register32::Ecx,
        Register32::Edx,
        Register32::Ebx,
    ];
    for (condition, table) in (0_u8..).zip(truth) {
        for destination in 0..8_u8 {
            for prefix in [false, true] {
                let mut code = vec![0x0f, 0x90 + condition, 0xc0 | destination];
                if prefix {
                    code.insert(0, 0x66);
                }
                let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
                for combination in 0..32_u32 {
                    let mut cpu = Cpu32::new(image.entry_point);
                    for reg in parents {
                        cpu.set_register(reg, 0xa5b6_c7d8);
                    }
                    cpu.eflags = 0x6212
                        | (combination & 1)
                        | ((combination & 2) << 1)
                        | ((combination & 4) << 4)
                        | ((combination & 8) << 4)
                        | ((combination & 16) << 7);
                    let parent = parents[usize::from(destination % 4)];
                    let shift = if destination < 4 { 0 } else { 8 };
                    let value = (table >> combination) & 1;
                    let mut expected = cpu;
                    expected.set_register(
                        parent,
                        (cpu.register(parent) & !(0xff << shift)) | value << shift,
                    );
                    expected.eip += u32::try_from(code.len()).unwrap();
                    let result = cpu.run(&mut image.memory, 1);
                    assert_eq!(result.reason, StopReason::InstructionLimit);
                    assert_eq!(result.instructions, 1);
                    assert_eq!(
                        cpu, expected,
                        "condition={condition} destination={destination} flags={combination} prefix={prefix}"
                    );
                }
            }
        }
    }
}

#[test]
fn memory_results_write_one_byte_without_reading_the_destination() {
    for address in [0x0040_2010_u32, 0x0040_2fff, u32::MAX] {
        for value in 0..=1_u8 {
            let mut code = vec![0x0f, 0x95, 0x05];
            code.extend_from_slice(&address.to_le_bytes());
            let mut image = load_pe32(&executable::pe32(&code), 4).unwrap();
            image
                .memory
                .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
                .unwrap();
            image
                .memory
                .write(u64::from(address) - 1, &[0xaa, 0xff])
                .unwrap();
            let page = u64::from(address & !0xfff);
            image
                .memory
                .protect(
                    page,
                    PAGE_SIZE,
                    Permissions {
                        read: false,
                        write: true,
                        execute: false,
                    },
                )
                .unwrap();
            let mut cpu = Cpu32::new(image.entry_point);
            cpu.eflags = 2 | (u32::from(1 - value) << 6);
            let mut expected = cpu;
            expected.eip += 7;
            let result = cpu.run(&mut image.memory, 1);
            assert_eq!(result.reason, StopReason::InstructionLimit);
            assert_eq!(result.instructions, 1);
            assert_eq!(cpu, expected);
            image
                .memory
                .protect(page, PAGE_SIZE, Permissions::READ)
                .unwrap();
            let mut bytes = [0; 2];
            image
                .memory
                .read(u64::from(address) - 1, &mut bytes)
                .unwrap();
            assert_eq!(bytes, [0xaa, value]);
        }
    }
    let mut image = load_pe32(&executable::pe32(&[0x64, 0x0f, 0x9e, 0x54, 0x90, 4]), 3).unwrap();
    image
        .memory
        .write(0x0040_200f, &[0xaa, 0xff, 0xbb])
        .unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_fs_base(0x0040_0000);
    cpu.set_register(Register32::Eax, 0x2000);
    cpu.set_register(Register32::Edx, 3);
    cpu.eflags = 0x802;
    let mut expected = cpu;
    expected.eip += 6;
    assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
    assert_eq!(cpu, expected);
    let mut bytes = [0; 3];
    image.memory.read(0x0040_200f, &mut bytes).unwrap();
    assert_eq!(bytes, [0xaa, 1, 0xbb]);
}

#[test]
fn write_faults_and_excluded_encodings_preserve_cpu_and_memory() {
    for address in [0x0040_2000_u32, 0x0040_3000] {
        for flags in [2, 0x42] {
            let mut code = vec![0x0f, 0x95, 0x05];
            code.extend_from_slice(&address.to_le_bytes());
            let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
            image
                .memory
                .protect(0x0040_2000, PAGE_SIZE, Permissions::READ)
                .unwrap();
            let mut cpu = Cpu32::new(image.entry_point);
            cpu.eflags = flags;
            let before = cpu;
            let result = cpu.run(&mut image.memory, 1);
            assert!(matches!(result.reason, StopReason::MemoryFault(_)));
            assert_eq!(result.instructions, 0);
            assert_eq!(cpu, before);
            let mut byte = [0];
            image.memory.read(0x0040_2000, &mut byte).unwrap();
            assert_eq!(byte, [17]);
        }
    }
    for code in [
        &[0x67, 0x0f, 0x95, 0][..],
        &[0xf3, 0x0f, 0x95, 0xc0],
        &[0xf2, 0x0f, 0x95, 0xc0],
        &[0x65, 0x0f, 0x95, 0],
        &[0xf0, 0x0f, 0x95, 0],
        &[0x0f, 0x45, 0xc0],
    ] {
        let mut image = load_pe32(&executable::pe32(code), 3).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        let before = cpu;
        let result = cpu.run(&mut image.memory, 1);
        assert_eq!(
            result.reason,
            if code[0] == 0xf0 {
                StopReason::InvalidInstruction
            } else {
                StopReason::UnsupportedInstruction
            }
        );
        assert_eq!(result.instructions, 0);
        assert_eq!(cpu, before);
    }
}

#[test]
fn condition_bytes_guest_matches_whole_and_single_instruction_execution() {
    let bytes = condition_bytes_executable::pe32();
    let mut whole = load_pe32(&bytes, 3).unwrap();
    let mut stepped = load_pe32(&bytes, 3).unwrap();
    let mut cpu = Cpu32::new(whole.entry_point);
    cpu.set_fs_base(0x0040_2000);
    let mut other = cpu;
    let result = cpu.run(&mut whole.memory, 20);
    assert_eq!(result.reason, StopReason::Breakpoint);
    assert_eq!(result.instructions, 10);
    let mut count = 0;
    for _ in 0..20 {
        let step = other.run(&mut stepped.memory, 1);
        count += step.instructions;
        if step.reason != StopReason::InstructionLimit {
            assert_eq!(step.reason, result.reason);
            break;
        }
    }
    assert_eq!(count, result.instructions);
    assert_eq!(other, cpu);
    assert_eq!(cpu.register(Register32::Eax), 0xaabb_0100);
    assert_eq!(cpu.register(Register32::Ebx), 0x100);
    assert_eq!(cpu.register(Register32::Ecx), 0x100);
    assert_eq!(cpu.eflags, 0x46);
    let mut values = [[0; 2]; 2];
    whole.memory.read(0x0040_2180, &mut values[0]).unwrap();
    stepped.memory.read(0x0040_2180, &mut values[1]).unwrap();
    assert_eq!(values, [[0, 1]; 2]);
}
