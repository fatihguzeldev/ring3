#[path = "support/executable.rs"]
mod executable;

use ring3_core::execution::{Cpu32, PAGE_SIZE, Permissions, Register32, StopReason, load_pe32};

#[test]
fn truncated_imul_forms_match_wide_product_and_signed_range() {
    for bits in [16, 32] {
        let mask = u32::MAX >> (32 - bits);
        let minimum = -(1_i128 << (bits - 1));
        let maximum = (1_i128 << (bits - 1)) - 1;
        let values = [minimum, maximum, -7, -1, 0, 1, 7];
        for form in 0..3 {
            let rights = if form == 2 {
                &[-128, -1, 0, 1, 2, 127][..]
            } else {
                &values[..]
            };
            for left in values {
                for &right in rights {
                    for source_memory in [false, true] {
                        let mut code = Vec::new();
                        if bits == 16 {
                            code.push(0x66);
                        }
                        code.extend_from_slice(match form {
                            0 => &[0x0f, 0xaf][..],
                            1 => &[0x69],
                            _ => &[0x6b],
                        });
                        code.push(if source_memory { 5 } else { 0xc3 });
                        if source_memory {
                            code.extend_from_slice(&0x0040_2ffc_u32.to_le_bytes());
                        }
                        let left_bits = u32::try_from(left & i128::from(mask)).unwrap();
                        let right_bits = u32::try_from(right & i128::from(mask)).unwrap();
                        let width = usize::try_from(bits / 8).unwrap();
                        if form != 0 {
                            code.extend_from_slice(
                                &right_bits.to_le_bytes()[..if form == 2 { 1 } else { width }],
                            );
                        }
                        let source = if form == 0 { right_bits } else { left_bits };
                        let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
                        image
                            .memory
                            .write(0x0040_2ffc, &source.to_le_bytes())
                            .unwrap();
                        image
                            .memory
                            .protect(0x0040_2000, PAGE_SIZE, Permissions::READ)
                            .unwrap();
                        let mut cpu = Cpu32::new(image.entry_point);
                        cpu.set_register(
                            Register32::Eax,
                            (0x1234_5678 & !mask) | if form == 0 { left_bits } else { 0x5678 },
                        );
                        cpu.set_register(Register32::Ebx, (0x8765_4321 & !mask) | source);
                        cpu.eflags = 0xced7;
                        let product = left * right;
                        let overflow = !(minimum..=maximum).contains(&product);
                        let mut wanted = cpu;
                        wanted.eip += u32::try_from(code.len()).unwrap();
                        wanted.eflags = (cpu.eflags & !0x801) | if overflow { 0x801 } else { 0 };
                        wanted.set_register(
                            Register32::Eax,
                            (cpu.register(Register32::Eax) & !mask)
                                | u32::try_from(product & i128::from(mask)).unwrap(),
                        );
                        assert_eq!(
                            cpu.run(&mut image.memory, 1).reason,
                            StopReason::InstructionLimit
                        );
                        assert_eq!(
                            cpu, wanted,
                            "bits={bits} form={form} left={left} right={right}"
                        );
                        let mut unchanged = [0; 4];
                        image.memory.read(0x0040_2ffc, &mut unchanged).unwrap();
                        assert_eq!(unchanged, source.to_le_bytes());
                    }
                }
            }
        }
    }
}

#[test]
fn source_aliases_and_fs_word_at_page_end_resume_identically() {
    let code = [
        0x69, 0xc9, 0xff, 0xff, 0xff, 0x1f, 0x64, 0x66, 0x6b, 0x05, 0, 0, 0, 0, 0xfd, 0xcc,
    ];
    let mut whole = load_pe32(&executable::pe32(&code), 3).unwrap();
    let mut stepped = load_pe32(&executable::pe32(&code), 3).unwrap();
    let mut cpu = Cpu32::new(whole.entry_point);
    cpu.set_register(Register32::Ecx, 8);
    cpu.set_register(Register32::Eax, 0xabcd_0000);
    cpu.set_fs_base(0x0040_2ffe);
    cpu.eflags = 0xced7;
    let mut other = cpu;
    for memory in [&mut whole.memory, &mut stepped.memory] {
        memory
            .write(0x0040_2ffe, &0xfffe_u16.to_le_bytes())
            .unwrap();
        memory
            .protect(0x0040_2000, PAGE_SIZE, Permissions::READ)
            .unwrap();
    }
    assert_eq!(
        cpu.run(&mut whole.memory, 10).reason,
        StopReason::Breakpoint
    );
    for _ in 0..2 {
        assert_eq!(
            other.run(&mut stepped.memory, 1).reason,
            StopReason::InstructionLimit
        );
    }
    assert_eq!(
        other.run(&mut stepped.memory, 1).reason,
        StopReason::Breakpoint
    );
    assert_eq!(cpu, other);
    assert_eq!(cpu.register(Register32::Ecx), 0xffff_fff8);
    assert_eq!(cpu.register(Register32::Eax), 0xabcd_0006);
    assert_eq!(cpu.eflags, 0xced7 & !0x801);
}

#[test]
fn unreadable_sources_and_unsupported_forms_leave_cpu_unchanged() {
    for address in [0x0040_2ffe_u32, 0xffff_fffe] {
        let mut code = vec![0x0f, 0xaf, 0x05];
        code.extend_from_slice(&address.to_le_bytes());
        let mut image = load_pe32(&executable::pe32(&code), 4).unwrap();
        if address == 0xffff_fffe {
            image
                .memory
                .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ)
                .unwrap();
        }
        let mut cpu = Cpu32::new(image.entry_point);
        cpu.set_register(Register32::Eax, 7);
        let before = cpu;
        let result = cpu.run(&mut image.memory, 1);
        assert!(matches!(result.reason, StopReason::MemoryFault(_)));
        assert_eq!(result.instructions, 0);
        assert_eq!(cpu, before);
    }
    for code in [
        &[0xf6, 0xe0][..],
        &[0x66, 0xf7, 0xf8],
        &[0x67, 0x0f, 0xaf, 0],
        &[0xf3, 0x0f, 0xaf, 0xc0],
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
    let mut image = load_pe32(&executable::pe32(&[0xf0, 0x0f, 0xaf, 0xc0]), 3).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    assert_eq!(
        cpu.run(&mut image.memory, 1).reason,
        StopReason::InvalidInstruction
    );
}
