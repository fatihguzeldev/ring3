use ring3_core::execution::{Cpu32, GuestMemory, PAGE_SIZE, Permissions, Register32, StopReason};

use super::flag_stack_executable;

fn encoding(width: u32, pop: bool) -> Vec<u8> {
    let mut code = if width == 2 { vec![0x66] } else { vec![] };
    code.push(if pop { 0x9d } else { 0x9c });
    code
}

fn setup(code: &[u8], flags: u32, stack: u32) -> (Cpu32, GuestMemory) {
    let mut memory = GuestMemory::new(8);
    memory
        .map_zeroed(0x1000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    memory.write(0x1000, code).unwrap();
    memory
        .protect(0x1000, PAGE_SIZE, Permissions::READ_EXECUTE)
        .unwrap();
    let mut cpu = Cpu32::new(0x1000);
    cpu.eflags = flags;
    cpu.set_register(Register32::Esp, stack);
    cpu.set_register(Register32::Eax, 0x1357_2468);
    cpu.set_register(Register32::Ebp, 0xabcd_ef01);
    cpu.set_fs_base(0x7000_0000);
    cpu.set_x87_control_word(0x027f);
    (cpu, memory)
}

fn step(cpu: &mut Cpu32, memory: &mut GuestMemory) {
    let result = cpu.run(memory, 1);
    assert_eq!(result.reason, StopReason::InstructionLimit);
    assert_eq!(result.instructions, 1);
}

fn expected_pop(old: u32, input: u32, width: u32) -> u32 {
    let mut result = old & !(1 << 16);
    for bit in 0..32 {
        let writable = matches!(bit, 0 | 2 | 4 | 6 | 7 | 8 | 10 | 11 | 14)
            || (bit == 9 && (old >> 12) & 3 == 3)
            || (width == 4 && matches!(bit, 18 | 21));
        if writable {
            result = (result & !(1 << bit)) | (input & (1 << bit));
        }
    }
    result
}

#[test]
fn pop_uses_current_privilege_and_width_masks_with_reserved_bits_preserved() {
    for width in [2, 4] {
        for iopl in 0..4 {
            for pattern in [2, 0xced7, 0x0039_0002, u32::MAX] {
                let old = (pattern & !0x0006_3100) | (iopl << 12);
                for input in [0, u32::MAX].into_iter().chain((0..32).map(|bit| 1 << bit)) {
                    let input = input & !0x0004_0100;
                    let code = encoding(width, true);
                    let (mut cpu, mut memory) = setup(&code, old, 0x8001);
                    memory
                        .map_zeroed(0x8000, PAGE_SIZE, Permissions::READ_WRITE)
                        .unwrap();
                    memory.write(0x8001, &input.to_le_bytes()).unwrap();
                    memory
                        .protect(0x8000, PAGE_SIZE, Permissions::READ)
                        .unwrap();
                    let mut expected = cpu;
                    expected.eflags = expected_pop(old, input, width);
                    expected.eip += u32::try_from(code.len()).unwrap();
                    expected.set_register(Register32::Esp, 0x8001 + width);
                    step(&mut cpu, &mut memory);
                    assert_eq!(cpu, expected, "width={width} old={old:x} input={input:x}");
                }
            }
        }
    }
}

#[test]
fn push_writes_only_the_flag_image_and_uses_full_esp_even_with_prefixes() {
    for width in [2, 4_u32] {
        for prefix in [vec![], vec![0x64], vec![0x67], vec![0x64, 0x67]] {
            for pattern in [2, 0xced7, 0x0039_0002, u32::MAX] {
                let flags = pattern & !0x0006_0100;
                for stack in [0, width, 0x9001] {
                    let mut code = prefix.clone();
                    code.extend(encoding(width, false));
                    let (mut cpu, mut memory) = setup(&code, flags, stack);
                    for page in [0, 0x8000, 0x9000, 0xffff_f000] {
                        memory
                            .map_zeroed(
                                page,
                                PAGE_SIZE,
                                Permissions {
                                    read: false,
                                    write: true,
                                    execute: false,
                                },
                            )
                            .unwrap();
                    }
                    let before = cpu;
                    assert_eq!(cpu.run(&mut memory, 0).instructions, 0);
                    assert_eq!(cpu, before);
                    step(&mut cpu, &mut memory);
                    let mut expected = before;
                    expected.eip += u32::try_from(code.len()).unwrap();
                    expected.set_register(Register32::Esp, stack.wrapping_sub(width));
                    assert_eq!(cpu, expected);
                    for page in [0, 0x8000, 0x9000, 0xffff_f000] {
                        memory.protect(page, PAGE_SIZE, Permissions::READ).unwrap();
                    }
                    let mut image = [0; 4];
                    memory
                        .read(
                            u64::from(stack.wrapping_sub(width)),
                            &mut image[..width as usize],
                        )
                        .unwrap();
                    let expected = if width == 2 {
                        flags & 0xffff
                    } else {
                        let mut bits = flags & 0x00ff_ffff;
                        bits &= !(1 << 16);
                        bits &= !(1 << 17);
                        bits
                    };
                    assert_eq!(u32::from_le_bytes(image), expected);
                }
            }
        }
    }
}

#[test]
fn trap_requests_and_unsupported_current_modes_preserve_state_and_read_ordering() {
    for width in [2, 4] {
        for mode in [1 << 8, 1 << 17, 1 << 18] {
            for pop in [false, true] {
                let (mut cpu, mut memory) = setup(&encoding(width, pop), mode | 2, 0x9000);
                let before = cpu;
                let result = cpu.run(&mut memory, 1);
                assert_eq!(result.reason, StopReason::UnsupportedInstruction);
                assert_eq!(result.instructions, 0);
                assert_eq!(cpu, before);
            }
        }
        for request in [1_u32 << 8, 1 << 18]
            .into_iter()
            .filter(|v| width == 4 || *v == 1 << 8)
        {
            let (mut cpu, mut memory) = setup(&encoding(width, true), 0x0001_0202, 0x8000);
            memory
                .map_zeroed(0x8000, PAGE_SIZE, Permissions::READ_WRITE)
                .unwrap();
            memory.write(0x8000, &request.to_le_bytes()).unwrap();
            memory
                .protect(0x8000, PAGE_SIZE, Permissions::NONE)
                .unwrap();
            let before = cpu;
            let fault = cpu.run(&mut memory, 1);
            assert!(matches!(fault.reason, StopReason::MemoryFault(_)));
            assert_eq!(cpu, before);
            memory
                .protect(0x8000, PAGE_SIZE, Permissions::READ)
                .unwrap();
            let result = cpu.run(&mut memory, 1);
            assert_eq!(result.reason, StopReason::UnsupportedInstruction);
            assert_eq!(result.instructions, 0);
            assert_eq!(cpu, before);
        }
    }
}

#[test]
fn partial_stack_access_faults_preserve_memory_flags_and_pointer_until_repaired() {
    for width in [2, 4_u32] {
        for pop in [false, true] {
            for absent in [false, true] {
                let address = 0x9000 - width / 2;
                let stack = if pop { address } else { address + width };
                let (mut cpu, mut memory) = setup(&encoding(width, pop), 0x10002, stack);
                memory
                    .map_zeroed(0x8000, PAGE_SIZE, Permissions::READ_WRITE)
                    .unwrap();
                memory.write(0x8000, &[0x02; 4096]).unwrap();
                if !absent {
                    memory
                        .map_zeroed(0x9000, PAGE_SIZE, Permissions::NONE)
                        .unwrap();
                }
                let before = cpu;
                let result = cpu.run(&mut memory, 1);
                assert!(matches!(result.reason, StopReason::MemoryFault(_)));
                assert_eq!(result.instructions, 0);
                assert_eq!(cpu, before);
                let mut bytes = [0; 4096];
                memory.read(0x8000, &mut bytes).unwrap();
                assert_eq!(bytes, [2; 4096]);
                if absent {
                    memory
                        .map_zeroed(0x9000, PAGE_SIZE, Permissions::READ_WRITE)
                        .unwrap();
                } else {
                    memory
                        .protect(0x9000, PAGE_SIZE, Permissions::READ_WRITE)
                        .unwrap();
                }
                step(&mut cpu, &mut memory);
                assert_eq!(
                    cpu.register(Register32::Esp),
                    if pop { stack + width } else { stack - width }
                );
            }
        }
    }
}

#[test]
fn word_reads_are_exact_and_guest_limit_wrap_is_only_between_instructions() {
    for width in [2, 4_u32] {
        let code = encoding(width, true);
        let stack = 0_u32.wrapping_sub(width);
        let (mut cpu, mut memory) = setup(&code, 0x0020_0002, stack);
        memory
            .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
            .unwrap();
        memory
            .write(u64::from(stack), &[2, 0, 0, 0][..width as usize])
            .unwrap();
        step(&mut cpu, &mut memory);
        assert_eq!(cpu.register(Register32::Esp), 0);
        assert_eq!(cpu.eflags, if width == 2 { 0x0020_0002 } else { 2 });
        for pop in [false, true] {
            let (mut cpu, mut memory) = setup(
                &encoding(width, pop),
                0x10002,
                if pop { u32::MAX } else { 1 },
            );
            memory
                .map_zeroed(0xffff_f000, PAGE_SIZE * 2, Permissions::READ_WRITE)
                .unwrap();
            let before = cpu;
            let result = cpu.run(&mut memory, 1);
            assert_eq!(
                result.reason,
                StopReason::MemoryFault(ring3_core::execution::MemoryError::AddressOverflow)
            );
            assert_eq!(result.instructions, 0);
            assert_eq!(cpu, before);
        }
    }
}

#[test]
fn forbidden_prefixes_stop_before_unmapped_stack_access() {
    for width in [2, 4] {
        for pop in [false, true] {
            for prefix in [0xf0, 0xf2, 0xf3, 0x65] {
                let mut code = vec![prefix];
                code.extend(encoding(width, pop));
                let (mut cpu, mut memory) = setup(&code, 2, 0x9000);
                let before = cpu;
                let result = cpu.run(&mut memory, 1);
                assert!(matches!(
                    result.reason,
                    StopReason::UnsupportedInstruction | StopReason::InvalidInstruction
                ));
                assert_eq!(result.instructions, 0);
                assert_eq!(cpu, before);
            }
        }
    }
}

#[test]
fn stack_access_requires_the_correct_permission_and_pop_prefixes_stay_flat() {
    for width in [2, 4] {
        for pop in [false, true] {
            for prefix in [vec![], vec![0x64], vec![0x67], vec![0x64, 0x67]] {
                let mut code = prefix;
                code.extend(encoding(width, pop));
                let (mut cpu, mut memory) =
                    setup(&code, 2, if pop { 0x8000 } else { 0x8000 + width });
                memory
                    .map_zeroed(
                        0x8000,
                        PAGE_SIZE,
                        Permissions {
                            read: !pop,
                            write: pop,
                            execute: false,
                        },
                    )
                    .unwrap();
                let before = cpu;
                let fault = cpu.run(&mut memory, 1);
                assert!(matches!(fault.reason, StopReason::MemoryFault(_)));
                assert_eq!(fault.instructions, 0);
                assert_eq!(cpu, before);
                memory
                    .protect(0x8000, PAGE_SIZE, Permissions::READ_WRITE)
                    .unwrap();
                step(&mut cpu, &mut memory);
                assert_eq!(cpu.eflags, 2);
                assert_eq!(
                    cpu.register(Register32::Esp),
                    if pop { 0x8000 + width } else { 0x8000 }
                );
            }
        }
    }
}

#[test]
fn authored_id_toggle_and_flag_restoration_agree_in_whole_and_single_step_runs() {
    use ring3_core::execution::{Process32, ProcessStop};
    let bytes = flag_stack_executable::pe32();
    let mut whole = Process32::load(&bytes, 32).unwrap();
    let mut stepped = Process32::load(&bytes, 32).unwrap();
    let result = whole.run(100);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (16, 0));
    for index in 0..16 {
        let result = stepped.run(1);
        assert_eq!((result.instructions, result.api_calls), (1, 0));
        assert_eq!(
            result.reason,
            ProcessStop::Stopped(if index == 15 {
                StopReason::Breakpoint
            } else {
                StopReason::InstructionLimit
            })
        );
    }
    assert_eq!(stepped.cpu, whole.cpu);
    for (register, expected) in [
        (Register32::Eax, 0),
        (Register32::Ebx, 0x0020_0002),
        (Register32::Ecx, 2),
        (Register32::Edx, 2),
        (Register32::Esp, 0x1001_0000),
    ] {
        assert_eq!(whole.cpu.register(register), expected);
    }
    assert_eq!(whole.cpu.eflags, 2);
}
