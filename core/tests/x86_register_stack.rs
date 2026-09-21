use ring3_core::execution::{Cpu32, GuestMemory, PAGE_SIZE, Permissions, Register32, StopReason};

#[path = "support/executable.rs"]
mod executable;
#[path = "support/register_stack_executable.rs"]
mod register_stack_executable;

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

fn encoding(width: u32, pop: bool) -> Vec<u8> {
    let mut code = if width == 2 { vec![0x66] } else { vec![] };
    code.push(if pop { 0x61 } else { 0x60 });
    code
}

fn setup(code: &[u8], stack: u32) -> (Cpu32, GuestMemory) {
    let mut memory = GuestMemory::new(24);
    memory
        .map_zeroed(0x1000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    memory.write(0x1000, code).unwrap();
    memory
        .protect(0x1000, PAGE_SIZE, Permissions::READ_EXECUTE)
        .unwrap();
    let mut cpu = Cpu32::new(0x1000);
    for (index, register) in REGISTERS.iter().enumerate() {
        cpu.set_register(
            *register,
            0x1234_5678_u32.wrapping_mul(u32::try_from(index + 1).unwrap()),
        );
    }
    cpu.set_register(Register32::Esp, stack);
    cpu.eflags = 0x00ed_ced7;
    cpu.set_fs_base(0x7000_0000);
    cpu.set_x87_control_word(0x027f);
    (cpu, memory)
}

fn step(cpu: &mut Cpu32, memory: &mut GuestMemory) {
    let result = cpu.run(memory, 1);
    assert_eq!(result.reason, StopReason::InstructionLimit);
    assert_eq!(result.instructions, 1);
}

#[test]
fn push_stores_independent_layout_and_original_stack_pointer_with_write_only_access() {
    for width in [2, 4_u32] {
        for prefix in [vec![], vec![0x64], vec![0x67], vec![0x64, 0x67]] {
            for stack in [0x9000, 0x9003, 0x10002] {
                let mut code = prefix.clone();
                code.extend(encoding(width, false));
                let (mut cpu, mut memory) = setup(&code, stack);
                memory
                    .map_zeroed(0x8000, PAGE_SIZE * 9, Permissions::READ_WRITE)
                    .unwrap();
                memory
                    .write(u64::from(stack - width * 8 - 1), &[0xa5; 34])
                    .unwrap();
                memory
                    .protect(
                        0x8000,
                        PAGE_SIZE * 9,
                        Permissions {
                            read: false,
                            write: true,
                            execute: false,
                        },
                    )
                    .unwrap();
                let before = cpu;
                let mut expected = vec![0xa5];
                for register in [
                    Register32::Edi,
                    Register32::Esi,
                    Register32::Ebp,
                    Register32::Esp,
                    Register32::Ebx,
                    Register32::Edx,
                    Register32::Ecx,
                    Register32::Eax,
                ] {
                    expected.extend_from_slice(
                        &before.register(register).to_le_bytes()[..width as usize],
                    );
                }
                expected.push(0xa5);
                assert_eq!(cpu.run(&mut memory, 0).instructions, 0);
                assert_eq!(cpu, before);
                step(&mut cpu, &mut memory);
                let mut wanted = before;
                wanted.eip += u32::try_from(code.len()).unwrap();
                wanted.set_register(Register32::Esp, stack - width * 8);
                assert_eq!(cpu, wanted);
                memory
                    .protect(0x8000, PAGE_SIZE * 9, Permissions::READ)
                    .unwrap();
                let mut actual = vec![0; expected.len()];
                memory
                    .read(u64::from(stack - width * 8 - 1), &mut actual)
                    .unwrap();
                assert_eq!(actual, expected);
            }
        }
    }
}

#[test]
fn pop_restores_order_preserves_upper_halves_and_ignores_saved_pointer() {
    let values = [
        0x1122_3344_u32,
        0x5566_7788,
        0x99aa_bbcc,
        0xdead_beef,
        0x1357_2468,
        0x2468_1357,
        0xfedc_ba98,
        0x7654_3210,
    ];
    for width in [2, 4_u32] {
        for prefix in [vec![], vec![0x64], vec![0x67], vec![0x64, 0x67]] {
            let mut code = prefix;
            code.extend(encoding(width, true));
            let (mut cpu, mut memory) = setup(&code, 0x8ff9);
            memory
                .map_zeroed(0x8000, PAGE_SIZE * 2, Permissions::READ_WRITE)
                .unwrap();
            for (slot, value) in values.iter().enumerate() {
                memory
                    .write(
                        0x8ff9 + slot as u64 * u64::from(width),
                        &value.to_le_bytes()[..width as usize],
                    )
                    .unwrap();
            }
            memory
                .protect(0x8000, PAGE_SIZE * 2, Permissions::READ)
                .unwrap();
            let mut expected = cpu;
            for (register, value) in [
                (Register32::Edi, values[0]),
                (Register32::Esi, values[1]),
                (Register32::Ebp, values[2]),
                (Register32::Ebx, values[4]),
                (Register32::Edx, values[5]),
                (Register32::Ecx, values[6]),
                (Register32::Eax, values[7]),
            ] {
                expected.set_register(
                    register,
                    if width == 2 {
                        (cpu.register(register) & 0xffff_0000) | (value & 0xffff)
                    } else {
                        value
                    },
                );
            }
            expected.set_register(Register32::Esp, 0x8ff9 + width * 8);
            expected.eip += u32::try_from(code.len()).unwrap();
            step(&mut cpu, &mut memory);
            assert_eq!(cpu, expected);
        }
    }
}

#[test]
fn late_stack_faults_are_atomic_and_retry_after_repair() {
    for width in [2, 4_u32] {
        for pop in [false, true] {
            for absent in [false, true] {
                let stack = if pop {
                    0x5000 - width * 4
                } else {
                    0x5000 + width * 4
                };
                let (mut cpu, mut memory) = setup(&encoding(width, pop), stack);
                let denied = if pop { 0x5000 } else { 0x4000 };
                for page in [0x4000, 0x5000] {
                    if !absent || page != denied {
                        memory
                            .map_zeroed(page, PAGE_SIZE, Permissions::READ_WRITE)
                            .unwrap();
                        memory.write(page, &[0xa5; 4096]).unwrap();
                    }
                }
                if !absent {
                    memory
                        .protect(denied, PAGE_SIZE, Permissions::NONE)
                        .unwrap();
                }
                let before = cpu;
                let result = cpu.run(&mut memory, 1);
                assert!(matches!(result.reason, StopReason::MemoryFault(_)));
                assert_eq!(result.instructions, 0);
                assert_eq!(cpu, before);
                let mut untouched = [0; 4096];
                memory
                    .read(if pop { 0x4000 } else { 0x5000 }, &mut untouched)
                    .unwrap();
                assert_eq!(untouched, [0xa5; 4096]);
                if absent {
                    memory
                        .map_zeroed(denied, PAGE_SIZE, Permissions::READ_WRITE)
                        .unwrap();
                } else {
                    memory
                        .protect(denied, PAGE_SIZE, Permissions::READ_WRITE)
                        .unwrap();
                }
                step(&mut cpu, &mut memory);
                assert_eq!(
                    cpu.register(Register32::Esp),
                    if pop {
                        stack + width * 8
                    } else {
                        stack - width * 8
                    }
                );
                assert_eq!(cpu.eflags, before.eflags);
            }
        }
    }
}

#[test]
fn stack_arithmetic_wraps_between_slots_and_pop_skips_an_overflowing_slot() {
    for width in [2, 4_u32] {
        let mut code = encoding(width, false);
        code.extend(encoding(width, true));
        let (mut cpu, mut memory) = setup(&code, width * 3);
        memory
            .map_zeroed(0, PAGE_SIZE, Permissions::READ_WRITE)
            .unwrap();
        memory
            .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
            .unwrap();
        let before = cpu;
        step(&mut cpu, &mut memory);
        assert_eq!(cpu.register(Register32::Esp), 0_u32.wrapping_sub(width * 5));
        step(&mut cpu, &mut memory);
        let mut expected = before;
        expected.eip += u32::try_from(code.len()).unwrap();
        assert_eq!(cpu, expected);

        let stack = if width == 2 {
            0xffff_fff9_u32
        } else {
            0xffff_fff2
        };
        let (mut cpu, mut memory) = setup(&encoding(width, true), stack);
        memory
            .map_zeroed(0, PAGE_SIZE, Permissions::READ_WRITE)
            .unwrap();
        memory
            .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
            .unwrap();
        for slot in 0..8 {
            if slot != 3 {
                memory
                    .write(
                        u64::from(stack.wrapping_add(slot * width)),
                        &[0x12; 4][..width as usize],
                    )
                    .unwrap();
            }
        }
        step(&mut cpu, &mut memory);
        assert_eq!(cpu.register(Register32::Esp), stack.wrapping_add(width * 8));
        for register in REGISTERS.into_iter().filter(|r| *r != Register32::Esp) {
            assert_eq!(
                cpu.register(register) & if width == 2 { 0xffff } else { u32::MAX },
                if width == 2 { 0x1212 } else { 0x1212_1212 }
            );
        }
    }
}

#[test]
fn accessed_scalar_crossing_the_guest_limit_faults_even_with_host_mapping_above_it() {
    for width in [2, 4] {
        for pop in [false, true] {
            let (mut cpu, mut memory) =
                setup(&encoding(width, pop), if pop { u32::MAX } else { 1 });
            memory
                .map_zeroed(0, PAGE_SIZE, Permissions::READ_WRITE)
                .unwrap();
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
            let mut bytes = [1; 8192];
            memory.read(0xffff_f000, &mut bytes).unwrap();
            assert_eq!(bytes, [0; 8192]);
        }
    }
}

#[test]
fn refused_prefixes_do_not_touch_the_stack() {
    for width in [2, 4] {
        for pop in [false, true] {
            for prefix in [0xf0, 0xf2, 0xf3, 0x65] {
                let mut code = vec![prefix];
                code.extend(encoding(width, pop));
                let (mut cpu, mut memory) = setup(&code, 0x9000);
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
fn authored_register_save_restore_agrees_in_whole_and_single_step_runs() {
    use ring3_core::execution::{Process32, ProcessStop};
    let bytes = register_stack_executable::pe32();
    let mut whole = Process32::load(&bytes, 32).unwrap();
    let mut stepped = Process32::load(&bytes, 32).unwrap();
    whole.cpu.eflags = 0xced7;
    stepped.cpu.eflags = 0xced7;
    let result = whole.run(100);
    assert_eq!(result.reason, ProcessStop::Stopped(StopReason::Breakpoint));
    assert_eq!((result.instructions, result.api_calls), (26, 0));
    for index in 0..26 {
        let result = stepped.run(1);
        assert_eq!((result.instructions, result.api_calls), (1, 0));
        assert_eq!(
            result.reason,
            ProcessStop::Stopped(if index == 25 {
                StopReason::Breakpoint
            } else {
                StopReason::InstructionLimit
            })
        );
        if index == 15 {
            assert_eq!(stepped.cpu.register(Register32::Eax), 0x1122_3344);
            assert_eq!(stepped.cpu.register(Register32::Edi), 0x90a0_b0c0);
            assert_eq!(stepped.cpu.register(Register32::Esp), 0x1001_0000);
        }
    }
    assert_eq!(stepped.cpu, whole.cpu);
    for (register, expected) in REGISTERS.into_iter().zip([
        0xa1a1_3344,
        0xa2a2_7788,
        0xa3a3_bbcc,
        0xa4a4_ff00,
        0x1001_0000,
        0xa5a5_3040,
        0xa6a6_7080,
        0xa7a7_b0c0,
    ]) {
        assert_eq!(whole.cpu.register(register), expected);
    }
    assert_eq!(whole.cpu.eflags, 0xced7);
}
