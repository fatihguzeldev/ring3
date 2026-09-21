#[path = "support/executable.rs"]
mod executable;
#[path = "support/string_stores_executable.rs"]
mod string_stores_executable;

use ring3_core::execution::{
    Cpu32, Permissions, Process32, ProcessStop, Register32, StopReason, load_pe32,
};

fn encoding(width: u32, repeat: bool, fs: bool) -> Vec<u8> {
    let mut code = Vec::new();
    if repeat {
        code.push(0xf3);
    }
    if fs {
        code.push(0x64);
    }
    if width == 2 {
        code.push(0x66);
    }
    code.push(if width == 1 { 0xaa } else { 0xab });
    code
}

#[test]
fn repeated_store_program_runs_whole_or_one_element_at_a_time() {
    for budget in [1, 40] {
        let mut p = Process32::load(&string_stores_executable::pe32(), 32).unwrap();
        p.cpu.set_fs_base(0x5000_0000);
        p.cpu.set_register(Register32::Esi, 0xdead_beef);
        p.cpu.eflags = 0xcad7;
        let mut count = 0;
        loop {
            let run = p.run(budget);
            count += run.instructions;
            assert_eq!(run.api_calls, 0);
            if run.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(run.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                break;
            }
            assert!(count < 40);
        }
        assert_eq!(count, 10);
        assert_eq!(p.cpu.register(Register32::Eax), 0x1234_5678);
        assert_eq!(p.cpu.register(Register32::Esi), 0xdead_beef);
        assert_eq!(p.cpu.register(Register32::Edi), 0x0040_208b);
        assert_eq!(p.cpu.register(Register32::Ecx), 0);
        assert_eq!(p.cpu.eflags, 0xcad7);
        let mut bytes = [0; 13];
        p.memory.read(0x0040_2080, &mut bytes).unwrap();
        assert_eq!(
            bytes,
            [
                0x78, 0x56, 0x34, 0x12, 0x78, 0x56, 0x34, 0x12, 0x78, 0x56, 0x78, 0xaa, 0xaa
            ]
        );
    }
}

#[test]
fn widths_directions_and_bare_or_repeat_store_only_accumulator_bytes() {
    for width in [1, 2, 4_u32] {
        for repeat in [false, true] {
            for reverse in [false, true] {
                for fs in [false, true] {
                    for count in [0, 1, 3_u32] {
                        let code = encoding(width, repeat, fs);
                        let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
                        let mut bytes = [0xaa; 256];
                        image.memory.write(0x0040_2000, &bytes).unwrap();
                        image
                            .memory
                            .protect(
                                0x0040_2000,
                                4096,
                                Permissions {
                                    write: true,
                                    ..Permissions::NONE
                                },
                            )
                            .unwrap();
                        let mut cpu = Cpu32::new(image.entry_point);
                        cpu.set_register(Register32::Eax, 0xa1b2_c3d4);
                        cpu.set_register(Register32::Esi, 0xdead_beef);
                        cpu.set_register(Register32::Edi, 0x0040_2089);
                        cpu.set_register(Register32::Ecx, count);
                        cpu.set_fs_base(0x6000_0000);
                        cpu.eflags = if reverse { 0xced7 } else { 0xcad7 };
                        let mut expected = cpu;
                        let elements = if repeat { count } else { 1 };
                        let mut offset = 137_usize;
                        for _ in 0..elements {
                            bytes[offset..offset + width as usize]
                                .copy_from_slice(&[0xd4, 0xc3, 0xb2, 0xa1][..width as usize]);
                            offset = if reverse {
                                offset - width as usize
                            } else {
                                offset + width as usize
                            };
                        }
                        expected.set_register(
                            Register32::Edi,
                            0x0040_2000 + u32::try_from(offset).unwrap(),
                        );
                        if repeat {
                            expected.set_register(Register32::Ecx, 0);
                        }
                        expected.eip += u32::try_from(code.len()).unwrap();
                        let run = cpu.run(&mut image.memory, u64::from(elements.max(1)));
                        assert_eq!(run.reason, StopReason::InstructionLimit);
                        assert_eq!(run.instructions, u64::from(elements.max(1)));
                        assert_eq!(cpu, expected);
                        image
                            .memory
                            .protect(0x0040_2000, 4096, Permissions::READ)
                            .unwrap();
                        let mut actual = [0; 256];
                        image.memory.read(0x0040_2000, &mut actual).unwrap();
                        assert_eq!(actual, bytes);
                    }
                }
            }
        }
    }
}

#[test]
fn partial_store_fault_keeps_completed_elements_and_resumes_without_partial_write() {
    for width in [1, 2, 4_u32] {
        for readonly in [false, true] {
            for budget in [1, 100] {
                let code = encoding(width, true, false);
                let mut image = load_pe32(&executable::pe32(&code), 6).unwrap();
                image
                    .memory
                    .map_zeroed(0x8000, 4096, Permissions::READ_WRITE)
                    .unwrap();
                if readonly {
                    image
                        .memory
                        .map_zeroed(0x9000, 4096, Permissions::READ)
                        .unwrap();
                }
                let destination = 0x9000 - width * 2 - (width - 1);
                let mut cpu = Cpu32::new(image.entry_point);
                cpu.set_register(Register32::Edi, destination);
                cpu.set_register(Register32::Ecx, 3);
                cpu.set_register(Register32::Eax, 0x1234_5678);
                cpu.eflags = 0xcad7;
                let mut steps = 0;
                loop {
                    let run = cpu.run(&mut image.memory, budget);
                    steps += run.instructions;
                    if matches!(run.reason, StopReason::MemoryFault(_)) {
                        break;
                    }
                    assert_eq!(run.reason, StopReason::InstructionLimit);
                    assert!(steps <= 2);
                }
                assert_eq!(steps, 2);
                assert_eq!(cpu.eip, image.entry_point);
                assert_eq!(cpu.register(Register32::Ecx), 1);
                assert_eq!(cpu.register(Register32::Edi), destination + width * 2);
                let mut bytes = vec![0; (width * 3 - 1) as usize];
                image
                    .memory
                    .read(u64::from(destination), &mut bytes)
                    .unwrap();
                let mut expected = [0x78, 0x56, 0x34, 0x12][..width as usize].repeat(2);
                expected.extend(vec![0; (width - 1) as usize]);
                assert_eq!(bytes, expected);
                let before = cpu;
                assert_eq!(cpu.run(&mut image.memory, 0).instructions, 0);
                assert_eq!(
                    cpu.run_until(&mut image.memory, 1, |_| true).reason,
                    StopReason::Intercepted
                );
                assert_eq!(cpu, before);
                if readonly {
                    image
                        .memory
                        .protect(0x9000, 4096, Permissions::READ_WRITE)
                        .unwrap();
                } else {
                    image
                        .memory
                        .map_zeroed(0x9000, 4096, Permissions::READ_WRITE)
                        .unwrap();
                }
                assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
                let mut expected_cpu = before;
                expected_cpu.eip += u32::try_from(code.len()).unwrap();
                expected_cpu.set_register(Register32::Ecx, 0);
                expected_cpu.set_register(Register32::Edi, destination + width * 3);
                assert_eq!(cpu, expected_cpu);
                let mut bytes = vec![0; (width * 3) as usize];
                image
                    .memory
                    .read(u64::from(destination), &mut bytes)
                    .unwrap();
                assert_eq!(bytes, [0x78, 0x56, 0x34, 0x12][..width as usize].repeat(3));
            }
        }
    }
}

#[test]
fn zero_and_huge_counts_are_bounded_but_invalid_forms_still_fail() {
    for width in [1, 2, 4] {
        let code = encoding(width, true, false);
        let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        cpu.set_register(Register32::Edi, u32::MAX);
        cpu.eflags = u32::MAX;
        let mut expected = cpu;
        expected.eip += u32::try_from(code.len()).unwrap();
        assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
        assert_eq!(cpu, expected);
        cpu.eip = image.entry_point;
        cpu.set_register(Register32::Edi, 0x0040_2080);
        cpu.set_register(Register32::Ecx, u32::MAX);
        let mut expected = cpu;
        expected.set_register(Register32::Ecx, u32::MAX - 2);
        expected.set_register(Register32::Edi, 0x0040_2080 - width * 2);
        assert_eq!(cpu.run(&mut image.memory, 2).instructions, 2);
        assert_eq!(cpu, expected);
    }
    for code in [
        vec![0x67, 0xaa],
        vec![0xf3, 0x67, 0xab],
        vec![0xf2, 0xaa],
        vec![0xf2, 0x66, 0xab],
        vec![0xf0, 0xab],
        vec![0x65, 0xaa],
    ] {
        let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        let before = cpu;
        let run = cpu.run(&mut image.memory, 1);
        assert!(matches!(
            run.reason,
            StopReason::UnsupportedInstruction | StopReason::InvalidInstruction
        ));
        assert_eq!(run.instructions, 0);
        assert_eq!(cpu, before);
    }
}

#[test]
fn indices_wrap_between_elements_but_an_element_never_wraps_its_write_span() {
    for width in [1, 2, 4_u32] {
        for reverse in [false, true] {
            let code = encoding(width, true, false);
            let mut image = load_pe32(&executable::pe32(&code), 5).unwrap();
            image
                .memory
                .map_zeroed(0, 4096, Permissions::READ_WRITE)
                .unwrap();
            image
                .memory
                .map_zeroed(0xffff_f000, 4096, Permissions::READ_WRITE)
                .unwrap();
            let mut cpu = Cpu32::new(image.entry_point);
            let destination = if reverse { 0 } else { u32::MAX - width + 1 };
            cpu.set_register(Register32::Edi, destination);
            cpu.set_register(Register32::Ecx, 2);
            cpu.set_register(Register32::Eax, 0x1234_5678);
            cpu.eflags = if reverse { 0xced7 } else { 0xcad7 };
            let step = if reverse { width.wrapping_neg() } else { width };
            assert_eq!(cpu.run(&mut image.memory, 2).instructions, 2);
            assert_eq!(
                cpu.register(Register32::Edi),
                destination.wrapping_add(step.wrapping_mul(2))
            );
            assert_eq!(cpu.register(Register32::Ecx), 0);
            for address in [destination, destination.wrapping_add(step)] {
                let mut bytes = vec![0; width as usize];
                image.memory.read(u64::from(address), &mut bytes).unwrap();
                assert_eq!(bytes, [0x78, 0x56, 0x34, 0x12][..width as usize]);
            }
            if width > 1 {
                cpu.eip = image.entry_point;
                cpu.set_register(Register32::Ecx, 1);
                cpu.set_register(Register32::Edi, u32::MAX);
                let before = cpu;
                let mut byte = [0];
                image.memory.read(u64::from(u32::MAX), &mut byte).unwrap();
                assert!(matches!(
                    cpu.run(&mut image.memory, 1).reason,
                    StopReason::MemoryFault(_)
                ));
                assert_eq!(cpu, before);
                let mut after = [0];
                image.memory.read(u64::from(u32::MAX), &mut after).unwrap();
                assert_eq!(after, byte);
            }
        }
    }
}
