#[path = "support/executable.rs"]
mod executable;

use ring3_core::execution::{Cpu32, PAGE_SIZE, Permissions, Register32, StopReason, load_pe32};

const REGISTERS: [Register32; 8] = [
    Register32::Eax,
    Register32::Ecx,
    Register32::Edx,
    Register32::Ebx,
    Register32::Esp,
    Register32::Ebp,
    Register32::Esi,
    Register32::Edi,
];

fn encoding(form: u8, modrm: u8) -> Vec<u8> {
    let mut code = if form == 0 { vec![0x66] } else { vec![] };
    code.extend_from_slice(&[0x0f, if form == 2 { 0xb7 } else { 0xb6 }, modrm]);
    code
}

#[test]
fn all_register_forms_zero_extend_without_changing_flags_or_unrelated_bits() {
    for form in 0..3 {
        for source in 0..8_u8 {
            for destination in 0..8_u8 {
                let code = encoding(form, 0xc0 | destination << 3 | source);
                let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
                let values: Vec<u32> = if form == 2 {
                    vec![0, 1, 0x7f, 0x80, 0xff, 0x100, 0x7fff, 0x8000, 0xffff]
                } else {
                    (0..256).collect()
                };
                for value in values {
                    let mut cpu = Cpu32::new(image.entry_point);
                    for reg in REGISTERS {
                        cpu.set_register(reg, 0xa5b6_c7d8);
                    }
                    let parent = usize::from(if form == 2 { source } else { source % 4 });
                    let shift = if form != 2 && source >= 4 { 8 } else { 0 };
                    cpu.set_register(REGISTERS[parent], 0xa5b6_0000 | value << shift);
                    cpu.eflags = 0xced7;
                    let mut expected = cpu;
                    let dest = REGISTERS[usize::from(destination)];
                    expected.set_register(
                        dest,
                        if form == 0 {
                            (cpu.register(dest) & 0xffff_0000) | value
                        } else {
                            value
                        },
                    );
                    expected.eip += u32::try_from(code.len()).unwrap();
                    let result = cpu.run(&mut image.memory, 1);
                    assert_eq!(result.reason, StopReason::InstructionLimit);
                    assert_eq!(result.instructions, 1);
                    assert_eq!(
                        cpu, expected,
                        "form={form} source={source} destination={destination} value={value}"
                    );
                }
            }
        }
    }
}

#[test]
fn memory_reads_use_source_width_including_fs_sib_and_top_address() {
    for form in 0..3 {
        let width = if form == 2 { 2 } else { 1 };
        for address in [0x0040_3000_u32 - width, u32::MAX - width + 1] {
            let mut code = encoding(form, 0x05);
            code.extend_from_slice(&address.to_le_bytes());
            let mut image = load_pe32(&executable::pe32(&code), 4).unwrap();
            image
                .memory
                .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
                .unwrap();
            image
                .memory
                .write(
                    u64::from(address),
                    &0xff80_u16.to_le_bytes()[..width as usize],
                )
                .unwrap();
            image
                .memory
                .protect(u64::from(address & !0xfff), PAGE_SIZE, Permissions::READ)
                .unwrap();
            let mut cpu = Cpu32::new(image.entry_point);
            cpu.set_register(Register32::Eax, 0xa5b6_c7d8);
            cpu.eflags = 0xced7;
            let mut expected = cpu;
            expected.set_register(
                Register32::Eax,
                match form {
                    0 => 0xa5b6_0080,
                    1 => 0x80,
                    _ => 0xff80,
                },
            );
            expected.eip += u32::try_from(code.len()).unwrap();
            assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
            assert_eq!(cpu, expected);
        }
        let mut code = encoding(form, 0x54);
        code.insert(0, 0x64);
        code.extend_from_slice(&[0x90, 4]);
        let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
        image.memory.write(0x0040_2010, &[0x80, 0xff]).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        cpu.set_fs_base(0x0040_0000);
        cpu.set_register(Register32::Eax, 0x2000);
        cpu.set_register(Register32::Edx, 3);
        cpu.eflags = 0xced7;
        let mut expected = cpu;
        expected.set_register(Register32::Edx, if form == 2 { 0xff80 } else { 0x80 });
        expected.eip += u32::try_from(code.len()).unwrap();
        assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
        assert_eq!(cpu, expected);
    }
}

#[test]
fn failed_extension_reads_and_excluded_encodings_preserve_the_entire_cpu() {
    for address in [0x0040_2000_u32, 0x0040_2fff, u32::MAX] {
        let mut code = encoding(2, 0x05);
        code.extend_from_slice(&address.to_le_bytes());
        let mut image = load_pe32(&executable::pe32(&code), 4).unwrap();
        image
            .memory
            .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
            .unwrap();
        image.memory.write(0xffff_ffff, &[0x80]).unwrap();
        image.memory.write(0x0040_2fff, &[0x80]).unwrap();
        if address == 0x0040_2000 {
            image
                .memory
                .protect(
                    0x0040_2000,
                    PAGE_SIZE,
                    Permissions {
                        read: false,
                        write: true,
                        execute: false,
                    },
                )
                .unwrap();
        }
        let mut cpu = Cpu32::new(image.entry_point);
        cpu.set_register(Register32::Eax, 0xa5b6_c7d8);
        cpu.eflags = 0xced7;
        let before = cpu;
        let result = cpu.run(&mut image.memory, 1);
        assert!(matches!(result.reason, StopReason::MemoryFault(_)));
        assert_eq!(result.instructions, 0);
        assert_eq!(cpu, before);
    }
    for code in [
        &[0x66, 0x0f, 0xb7, 0xc0][..],
        &[0x0f, 0xbe, 0xc0],
        &[0x67, 0x0f, 0xb6, 0],
        &[0xf3, 0x0f, 0xb6, 0xc0],
        &[0xf2, 0x0f, 0xb6, 0xc0],
        &[0x65, 0x0f, 0xb6, 0],
    ] {
        let mut image = load_pe32(&executable::pe32(code), 3).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        let before = cpu;
        assert_eq!(
            cpu.run(&mut image.memory, 1).reason,
            StopReason::UnsupportedInstruction
        );
        assert_eq!(cpu, before);
    }
    let mut image = load_pe32(&executable::pe32(&[0xf0, 0x0f, 0xb6, 0]), 3).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    let before = cpu;
    assert_eq!(
        cpu.run(&mut image.memory, 1).reason,
        StopReason::InvalidInstruction
    );
    assert_eq!(cpu, before);
}

#[path = "support/zero_extend_executable.rs"]
mod zero_extend_executable;

#[test]
fn zero_extension_guest_matches_whole_and_single_instruction_execution() {
    let bytes = zero_extend_executable::pe32();
    let mut whole = load_pe32(&bytes, 3).unwrap();
    let mut stepped = load_pe32(&bytes, 3).unwrap();
    let mut cpu = Cpu32::new(whole.entry_point);
    cpu.eflags = 0xced7;
    cpu.set_fs_base(0x0040_2000);
    let mut other = cpu;
    let result = cpu.run(&mut whole.memory, 20);
    assert_eq!(result.reason, StopReason::Breakpoint);
    assert_eq!(result.instructions, 7);
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
    assert_eq!(cpu.register(Register32::Eax), 0xff);
    assert_eq!(cpu.register(Register32::Ebx), 0x1234_00ff);
    assert_eq!(cpu.register(Register32::Ecx), 0xff);
    assert_eq!(cpu.register(Register32::Edx), 0xff80);
    assert_eq!(cpu.eflags, 0xced7);
}
