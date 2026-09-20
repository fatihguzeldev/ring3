#[path = "support/executable.rs"]
mod executable;

use ring3_core::execution::{Cpu32, Permissions, Register32, StopReason, load_pe32};

fn forms(bits: u32, decrement: bool) -> Vec<(Vec<u8>, bool, u32)> {
    let prefix = if bits == 16 { vec![0x66] } else { vec![] };
    let opcode = if bits == 8 { 0xfe } else { 0xff };
    let group = u8::from(decrement) * 8;
    let mut register = prefix.clone();
    register.extend_from_slice(&[opcode, 0xc0 + group]);
    let mut memory = prefix.clone();
    memory.extend_from_slice(&[opcode, 0x05 + group, 0, 0x20, 0x40, 0]);
    let mut forms = vec![(register, false, 0), (memory, true, 0)];
    if bits == 8 {
        forms.push((vec![opcode, 0xc4 + group], false, 8));
    } else {
        let mut short = prefix;
        short.push(0x40 + group);
        forms.push((short, false, 0));
    }
    forms
}

#[test]
fn all_widths_and_forms_preserve_carry_and_update_other_arithmetic_flags() {
    for bits in [8, 16, 32] {
        let mask = u32::MAX >> (32 - bits);
        let sign = 1_u32 << (bits - 1);
        for (decrement, cases) in [
            (
                false,
                [
                    (0, 1, 0),
                    (mask, 0, 0x54),
                    (sign - 1, sign, if bits == 8 { 0x890 } else { 0x894 }),
                ],
            ),
            (
                true,
                [
                    (1, 0, 0x44),
                    (0, mask, 0x94),
                    (sign, sign - 1, if bits == 8 { 0x810 } else { 0x814 }),
                ],
            ),
        ] {
            for (input, output, flags) in cases {
                for carry in [0, 1] {
                    for (code, memory_destination, shift) in forms(bits, decrement) {
                        let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
                        let initial = (0x1234_5678 & !(mask << shift)) | (input << shift);
                        image
                            .memory
                            .write(0x0040_2000, &initial.to_le_bytes())
                            .unwrap();
                        let mut cpu = Cpu32::new(image.entry_point);
                        cpu.set_register(Register32::Eax, initial);
                        cpu.eflags = 0xced6 | carry;
                        let run = cpu.run(&mut image.memory, 1);
                        assert_eq!(run.instructions, 1, "{code:02x?} {:?}", run.reason);
                        let expected = (initial & !(mask << shift)) | (output << shift);
                        let mut bytes = [0; 4];
                        image.memory.read(0x0040_2000, &mut bytes).unwrap();
                        assert_eq!(
                            cpu.register(Register32::Eax),
                            if memory_destination {
                                initial
                            } else {
                                expected
                            }
                        );
                        assert_eq!(
                            u32::from_le_bytes(bytes),
                            if memory_destination {
                                expected
                            } else {
                                initial
                            }
                        );
                        assert_eq!(
                            cpu.eflags & 0x8d5,
                            flags | carry,
                            "{bits} {input:x} {code:02x?}"
                        );
                        assert_eq!(cpu.eflags & !0x8d5, 0xced6 & !0x8d5);
                    }
                }
            }
        }
    }
}

#[test]
fn failed_increment_and_unsupported_prefixes_preserve_cpu_and_memory() {
    for address in [0x0040_2000_u32, 0x0040_2fff, u32::MAX] {
        let mut code = vec![0xff, 0x05];
        code.extend_from_slice(&address.to_le_bytes());
        let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
        image
            .memory
            .protect(0x0040_2000, 4096, Permissions::READ)
            .unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        cpu.eflags = 0xced7;
        let before = cpu;
        assert!(matches!(
            cpu.run(&mut image.memory, 1).reason,
            StopReason::MemoryFault(_)
        ));
        assert_eq!(cpu, before);
        let mut byte = [0];
        image.memory.read(0x0040_2000, &mut byte).unwrap();
        assert_eq!(byte, [17]);
    }
    for code in [
        &[0xf0, 0xff, 0x05, 0, 0x20, 0x40, 0][..],
        &[0xf3, 0x40][..],
        &[0x67, 0xff, 0][..],
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
}

#[test]
fn fs_increment_and_word_counter_resume_with_carry_intact() {
    let code = [
        0x64, 0xfe, 0x00, 0x66, 0xb9, 3, 0, 0x66, 0x49, 0x75, 0xfc, 0xcc,
    ];
    let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_fs_base(0x0040_2000);
    cpu.eflags = 3;
    let mut instructions = 0;
    loop {
        let run = cpu.run(&mut image.memory, 1);
        instructions += run.instructions;
        if run.reason != StopReason::InstructionLimit {
            assert_eq!(run.reason, StopReason::Breakpoint);
            break;
        }
    }
    assert_eq!(instructions, 9);
    assert_eq!(cpu.register(Register32::Ecx), 0);
    assert_eq!(cpu.eflags & 1, 1);
    let mut value = [0; 4];
    image.memory.read(0x0040_2000, &mut value).unwrap();
    assert_eq!(value, [18, 0, 0, 0]);
}
