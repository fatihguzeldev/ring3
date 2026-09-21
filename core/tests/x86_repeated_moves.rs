#[path = "support/executable.rs"]
mod executable;
#[path = "support/repeated_moves_executable.rs"]
mod repeated_moves_executable;

use ring3_core::execution::{
    Cpu32, PAGE_SIZE, Permissions, Process32, ProcessStop, Register32, StopReason, load_pe32,
};

fn encoding(width: u32, fs: bool) -> Vec<u8> {
    let mut code = vec![0xf3];
    if fs {
        code.push(0x64);
    }
    if width == 2 {
        code.push(0x66);
    }
    code.push(if width == 1 { 0xa4 } else { 0xa5 });
    code
}

#[test]
fn repeats_match_sequential_copy_oracle_for_width_direction_fs_and_overlap() {
    for width in [1, 2, 4_u32] {
        for reverse in [false, true] {
            for fs in [false, true] {
                let code = encoding(width, fs);
                let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
                for shift in [-4_i32, -1, 0, 1, 4, 128] {
                    for count in [0, 1, 5_u32] {
                        let mut expected_bytes = [0; 1024];
                        for (index, byte) in expected_bytes.iter_mut().enumerate() {
                            *byte = u8::try_from(index % 251).unwrap();
                        }
                        image.memory.write(0x0040_2000, &expected_bytes).unwrap();
                        let (mut source, mut destination) =
                            (256_usize, usize::try_from(256 + shift).unwrap());
                        let mut cpu = Cpu32::new(image.entry_point);
                        cpu.set_fs_base(0x0040_2000);
                        cpu.set_register(Register32::Esi, if fs { 256 } else { 0x0040_2100 });
                        cpu.set_register(
                            Register32::Edi,
                            0x0040_2000 + u32::try_from(destination).unwrap(),
                        );
                        cpu.set_register(Register32::Ecx, count);
                        cpu.eflags = if reverse { 0xced7 } else { 0xcad7 };
                        let mut expected = cpu;
                        let bytes = width as usize;
                        for _ in 0..count {
                            let mut captured = [0; 4];
                            captured[..bytes]
                                .copy_from_slice(&expected_bytes[source..source + bytes]);
                            expected_bytes[destination..destination + bytes]
                                .copy_from_slice(&captured[..bytes]);
                            if reverse {
                                source -= bytes;
                                destination -= bytes;
                            } else {
                                source += bytes;
                                destination += bytes;
                            }
                        }
                        expected.set_register(
                            Register32::Esi,
                            u32::try_from(source).unwrap() + if fs { 0 } else { 0x0040_2000 },
                        );
                        expected.set_register(
                            Register32::Edi,
                            0x0040_2000 + u32::try_from(destination).unwrap(),
                        );
                        expected.set_register(Register32::Ecx, 0);
                        expected.eip += u32::try_from(code.len()).unwrap();
                        assert_eq!(
                            cpu.run(&mut image.memory, u64::from(count.max(1)))
                                .instructions,
                            u64::from(count.max(1))
                        );
                        assert_eq!(cpu, expected);
                        let mut actual = [0; 1024];
                        image.memory.read(0x0040_2000, &mut actual).unwrap();
                        assert_eq!(actual, expected_bytes);
                    }
                }
            }
        }
    }
}

#[test]
fn partial_read_and_write_faults_retain_only_completed_elements_and_resume() {
    for width in [1, 2, 4_u32] {
        for write_fault in [false, true] {
            for budget in [1, 100] {
                let code = encoding(width, false);
                let mut image = load_pe32(&executable::pe32(&code), 6).unwrap();
                image
                    .memory
                    .map_zeroed(0x8000, PAGE_SIZE, Permissions::READ_WRITE)
                    .unwrap();
                image
                    .memory
                    .map_zeroed(0x9000, PAGE_SIZE, Permissions::NONE)
                    .unwrap();
                let edge = 0x9000 - width * 2 - (width - 1);
                let (source, destination) = if write_fault {
                    (0x0040_2080, edge)
                } else {
                    (edge, 0x0040_2080)
                };
                let data: Vec<_> = (1..=width * 2).map(|n| u8::try_from(n).unwrap()).collect();
                image.memory.write(u64::from(source), &data).unwrap();
                if write_fault {
                    image
                        .memory
                        .protect(0x0040_2000, PAGE_SIZE, Permissions::READ)
                        .unwrap();
                }
                let mut cpu = Cpu32::new(image.entry_point);
                cpu.set_register(Register32::Esi, source);
                cpu.set_register(Register32::Edi, destination);
                cpu.set_register(Register32::Ecx, 3);
                cpu.eflags = 0xcad7;
                let mut steps = 0;
                loop {
                    let result = cpu.run(&mut image.memory, budget);
                    steps += result.instructions;
                    if matches!(result.reason, StopReason::MemoryFault(_)) {
                        break;
                    }
                    assert_eq!(result.reason, StopReason::InstructionLimit);
                    assert!(steps <= 2);
                }
                assert_eq!(steps, 2);
                assert_eq!(cpu.eip, image.entry_point);
                assert_eq!(cpu.register(Register32::Ecx), 1);
                assert_eq!(cpu.register(Register32::Esi), source + width * 2);
                assert_eq!(cpu.register(Register32::Edi), destination + width * 2);
                assert_eq!(cpu.eflags, 0xcad7);
                let mut copied = vec![0; data.len()];
                image
                    .memory
                    .read(u64::from(destination), &mut copied)
                    .unwrap();
                assert_eq!(copied, data);
                if width > 1 {
                    let mut tail = vec![0xff; (width - 1) as usize];
                    image
                        .memory
                        .read(u64::from(destination + width * 2), &mut tail)
                        .unwrap();
                    assert_eq!(tail, vec![0; (width - 1) as usize]);
                }
                let before = cpu;
                assert_eq!(cpu.run(&mut image.memory, 0).instructions, 0);
                assert_eq!(
                    cpu.run_until(&mut image.memory, 1, |_| true).reason,
                    StopReason::Intercepted
                );
                assert_eq!(cpu, before);
                image
                    .memory
                    .protect(0x9000, PAGE_SIZE, Permissions::READ_WRITE)
                    .unwrap();
                assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
                assert_eq!(
                    cpu.eip,
                    image.entry_point + u32::try_from(code.len()).unwrap()
                );
                assert_eq!(cpu.register(Register32::Ecx), 0);
            }
        }
    }
}

#[test]
fn zero_counts_and_huge_counts_are_bounded_and_keep_flags() {
    for width in [1, 2, 4] {
        let code = encoding(width, false);
        let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        cpu.set_register(Register32::Esi, u32::MAX);
        cpu.set_register(Register32::Edi, u32::MAX);
        cpu.eflags = u32::MAX;
        let mut expected = cpu;
        expected.eip += u32::try_from(code.len()).unwrap();
        assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
        assert_eq!(cpu, expected);
        cpu.eip = image.entry_point;
        cpu.set_register(Register32::Esi, 0x0040_2080);
        cpu.set_register(Register32::Edi, 0x0040_2180);
        cpu.set_register(Register32::Ecx, u32::MAX);
        assert_eq!(cpu.run(&mut image.memory, 2).instructions, 2);
        assert_eq!(cpu.eip, image.entry_point);
        assert_eq!(cpu.register(Register32::Ecx), u32::MAX - 2);
        assert_eq!(cpu.register(Register32::Esi), 0x0040_2080 - width * 2);
        assert_eq!(cpu.register(Register32::Edi), 0x0040_2180 - width * 2);
        assert_eq!(cpu.eflags, u32::MAX);
    }
}

#[test]
fn indices_wrap_between_elements_without_wrapping_an_element_span() {
    for width in [1, 2, 4_u32] {
        for reverse in [false, true] {
            for source_wrap in [false, true] {
                let mut image = load_pe32(&executable::pe32(&encoding(width, false)), 5).unwrap();
                image
                    .memory
                    .map_zeroed(0, PAGE_SIZE, Permissions::READ_WRITE)
                    .unwrap();
                image
                    .memory
                    .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
                    .unwrap();
                let edge = if reverse { 0 } else { u32::MAX - width + 1 };
                let step = if reverse { width.wrapping_neg() } else { width };
                let (source, destination) = if source_wrap {
                    (edge, 0x0040_2080)
                } else {
                    (0x0040_2080, edge)
                };
                for index in 0..2_u32 {
                    image
                        .memory
                        .write(
                            u64::from(source.wrapping_add(step.wrapping_mul(index))),
                            &(index + 1).to_le_bytes()[..width as usize],
                        )
                        .unwrap();
                }
                let mut cpu = Cpu32::new(image.entry_point);
                cpu.set_register(Register32::Esi, source);
                cpu.set_register(Register32::Edi, destination);
                cpu.set_register(Register32::Ecx, 2);
                cpu.eflags = if reverse { 0x402 } else { 2 };
                assert_eq!(cpu.run(&mut image.memory, 2).instructions, 2);
                assert_eq!(cpu.register(Register32::Ecx), 0);
                assert_eq!(
                    cpu.register(Register32::Esi),
                    source.wrapping_add(step.wrapping_mul(2))
                );
                assert_eq!(
                    cpu.register(Register32::Edi),
                    destination.wrapping_add(step.wrapping_mul(2))
                );
                for index in 0..2_u32 {
                    let mut copied = [0; 4];
                    image
                        .memory
                        .read(
                            u64::from(destination.wrapping_add(step.wrapping_mul(index))),
                            &mut copied[..width as usize],
                        )
                        .unwrap();
                    assert_eq!(u32::from_le_bytes(copied), index + 1);
                }
            }
        }
    }
}

#[test]
fn mixed_repeat_guest_agrees_whole_per_element_and_process_layer() {
    let bytes = repeated_moves_executable::pe32();
    for budget in [1, 50] {
        let mut p = Process32::load(&bytes, 32).unwrap();
        p.cpu.eflags = 0xcad7;
        let mut steps = 0;
        loop {
            let result = p.run(budget);
            steps += result.instructions;
            assert_eq!(result.api_calls, 0);
            if result.reason != ProcessStop::Stopped(StopReason::InstructionLimit) {
                assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
                break;
            }
            assert!(steps < 50);
        }
        assert_eq!(steps, 10);
        assert_eq!(p.cpu.register(Register32::Ecx), 0);
        assert_eq!(p.cpu.register(Register32::Esi), 0x0040_208b);
        assert_eq!(p.cpu.register(Register32::Edi), 0x0040_20cb);
        assert_eq!(p.cpu.eflags, 0xcad7);
        let mut copied = [0; 11];
        p.memory.read(0x0040_20c0, &mut copied).unwrap();
        assert_eq!(copied, [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]);
    }
}
