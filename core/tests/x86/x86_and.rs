use super::executable;

use ring3_core::execution::{Cpu32, PAGE_SIZE, Permissions, Register32, StopReason, load_pe32};

#[test]
fn dword_forms_mask_bits_and_set_logical_flags() {
    for (left, right) in [
        (0xffff_ffff_u32, 0x8000_0003_u32),
        (0x8000_0000, 1),
        (0x1234_5678, u32::MAX),
    ] {
        for (mut code, immediate, memory_destination) in [
            (vec![0x25], true, false),
            (vec![0x81, 0xe0], true, false),
            (vec![0x21, 0xd8], false, false),
            (vec![0x23, 0x05, 4, 0x20, 0x40, 0], false, false),
            (vec![0x21, 0x1d, 0, 0x20, 0x40, 0], false, true),
            (vec![0x81, 0x25, 0, 0x20, 0x40, 0], true, true),
        ] {
            if immediate {
                code.extend_from_slice(&right.to_le_bytes());
            }
            let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
            image
                .memory
                .write(0x0040_2000, &left.to_le_bytes())
                .unwrap();
            image
                .memory
                .write(0x0040_2004, &right.to_le_bytes())
                .unwrap();
            let mut cpu = Cpu32::new(image.entry_point);
            cpu.set_register(Register32::Eax, left);
            cpu.set_register(Register32::Ebx, right);
            cpu.eflags = 0xced7;
            assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
            let expected = left & right;
            let flags = (u32::from(expected.count_ones() == 0) << 6)
                | (u32::from(expected.to_le_bytes()[0].count_ones() % 2 == 0) << 2)
                | ((expected >> 31) << 7);
            assert_eq!(cpu.eflags, (0xced7 & !0x8d5) | flags);
            assert_eq!(cpu.register(Register32::Ebx), right);
            assert_eq!(
                cpu.register(Register32::Eax),
                if memory_destination { left } else { expected }
            );
            let mut bytes = [0; 4];
            image.memory.read(0x0040_2000, &mut bytes).unwrap();
            assert_eq!(
                u32::from_le_bytes(bytes),
                if memory_destination { expected } else { left }
            );
        }
    }
}

#[test]
fn sign_extended_immediates_high_bytes_and_fs_memory_resume_consistently() {
    let code = [
        0xb8, 0x7f, 0xff, 0x34, 0x12, 0x20, 0xc4, 0x83, 0xe0, 0xf0, 0x64, 0x83, 0x25, 0, 0, 0, 0,
        0x80, 0xcc,
    ];
    let mut whole = load_pe32(&executable::pe32(&code), 3).unwrap();
    let mut stepped = load_pe32(&executable::pe32(&code), 3).unwrap();
    let mut cpu = Cpu32::new(whole.entry_point);
    cpu.set_fs_base(0x0040_2000);
    let mut other = cpu;
    for memory in [&mut whole.memory, &mut stepped.memory] {
        memory.write(0x0040_2000, &u32::MAX.to_le_bytes()).unwrap();
    }
    let result = cpu.run(&mut whole.memory, 20);
    assert_eq!(result.reason, StopReason::Breakpoint);
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
    assert_eq!(cpu.register(Register32::Eax), 0x1234_7f70);
    for memory in [&whole.memory, &stepped.memory] {
        let mut bytes = [0; 4];
        memory.read(0x0040_2000, &mut bytes).unwrap();
        assert_eq!(bytes, 0xffff_ff80_u32.to_le_bytes());
    }
}

#[test]
fn faulting_and_writes_and_unsupported_prefixes_are_atomic() {
    for address in [0x0040_2ffe_u32, 0xffff_fffe] {
        let mut code = vec![0x83, 0x25];
        code.extend_from_slice(&address.to_le_bytes());
        code.push(0);
        let mut image = load_pe32(&executable::pe32(&code), 5).unwrap();
        if address == 0x0040_2ffe {
            image
                .memory
                .map_zeroed(0x0040_3000, PAGE_SIZE, Permissions::READ)
                .unwrap();
        } else {
            image
                .memory
                .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
                .unwrap();
        }
        image.memory.write(u64::from(address), &[0xff; 2]).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        cpu.eflags = 0xced7;
        let before = cpu;
        let result = cpu.run(&mut image.memory, 1);
        assert!(matches!(result.reason, StopReason::MemoryFault(_)));
        assert_eq!(result.instructions, 0);
        assert_eq!(cpu, before);
        let mut bytes = [0; 2];
        image.memory.read(u64::from(address), &mut bytes).unwrap();
        assert_eq!(bytes, [0xff; 2]);
    }
    for code in [
        &[0xf0, 0x21, 0x18][..],
        &[0xf3, 0x21, 0xd8],
        &[0x67, 0x21, 0],
        &[0x82, 0xe0, 0],
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
