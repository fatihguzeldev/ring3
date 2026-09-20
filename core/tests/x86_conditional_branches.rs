#[path = "support/executable.rs"]
mod executable;

use ring3_core::execution::{
    Cpu32, GuestMemory, PAGE_SIZE, Permissions, Register32, StopReason, load_pe32,
};

#[test]
fn all_flag_conditions_follow_the_truth_table_in_short_and_near_forms() {
    // truth bits enumerate cf,pf,zf,sf,of in that order, independent of eflags layout.
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
    for (condition, table) in (0_u8..).zip(truth) {
        for combination in 0..32_u32 {
            for near in [false, true] {
                let code = if near {
                    vec![0x0f, 0x80 + condition, 7, 0, 0, 0]
                } else {
                    vec![0x70 + condition, 7]
                };
                let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
                let mut cpu = Cpu32::new(image.entry_point);
                cpu.set_register(Register32::Eax, 42);
                cpu.eflags = 0x6212
                    | (combination & 1)
                    | ((combination & 2) << 1)
                    | ((combination & 4) << 4)
                    | ((combination & 8) << 4)
                    | ((combination & 16) << 7);
                let mut expected = cpu;
                expected.eip += u32::try_from(code.len()).unwrap()
                    + if table & (1 << combination) != 0 {
                        7
                    } else {
                        0
                    };
                let result = cpu.run(&mut image.memory, 1);
                assert_eq!(
                    result.reason,
                    StopReason::InstructionLimit,
                    "{condition:x} {combination:x} {near}"
                );
                assert_eq!(result.instructions, 1);
                assert_eq!(cpu, expected, "{condition:x} {combination:x} {near}");
            }
        }
    }
}

#[test]
fn signed_comparison_branches_use_overflow_and_loops_resume() {
    for (left, right) in [(i32::MIN, 1_i32), (i32::MAX, -1), (-1, 0), (0, 0), (1, -1)] {
        let mut image = load_pe32(
            &executable::pe32(&[0x39, 0xd8, 0x7d, 5, 0xb9, 42, 0, 0, 0, 0xcc]),
            3,
        )
        .unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        cpu.set_register(Register32::Eax, left.cast_unsigned());
        cpu.set_register(Register32::Ebx, right.cast_unsigned());
        assert_eq!(
            cpu.run(&mut image.memory, 20).reason,
            StopReason::Breakpoint
        );
        assert_eq!(
            cpu.register(Register32::Ecx),
            if left >= right { 0 } else { 42 }
        );
    }
    let code = [0xb9, 0xfd, 0xff, 0xff, 0xff, 0x41, 0x7c, 0xfd, 0xcc];
    let mut a = load_pe32(&executable::pe32(&code), 3).unwrap();
    let mut b = load_pe32(&executable::pe32(&code), 3).unwrap();
    let mut whole = Cpu32::new(a.entry_point);
    let mut stepped = whole;
    let result = whole.run(&mut a.memory, 30);
    assert_eq!(result.reason, StopReason::Breakpoint);
    let mut count = 0;
    for _ in 0..30 {
        let step = stepped.run(&mut b.memory, 1);
        count += step.instructions;
        if step.reason != StopReason::InstructionLimit {
            assert_eq!(step.reason, result.reason);
            break;
        }
    }
    assert_eq!(count, result.instructions);
    assert_eq!(stepped, whole);
    assert_eq!(whole.register(Register32::Ecx), 0);
}

#[test]
fn taken_targets_wrap_and_fetch_faults_follow_the_completed_branch() {
    let mut memory = GuestMemory::new(1);
    memory
        .map_zeroed(0xffff_f000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    memory.write(0xffff_fffe, &[0x7d, 0xff]).unwrap();
    memory
        .protect(0xffff_f000, PAGE_SIZE, Permissions::READ_EXECUTE)
        .unwrap();
    let mut cpu = Cpu32::new(0xffff_fffe);
    assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
    assert_eq!(cpu.eip, u32::MAX);
    let mut image = load_pe32(&executable::pe32(&[0x0f, 0x8d, 0xfa, 0xef, 0xbf, 0xff]), 3).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
    assert_eq!(cpu.eip, 0);
    let before = cpu;
    let result = cpu.run(&mut image.memory, 1);
    assert!(matches!(result.reason, StopReason::MemoryFault(_)));
    assert_eq!(result.instructions, 0);
    assert_eq!(cpu, before);
}

#[test]
fn unsupported_branch_forms_and_prefixes_preserve_cpu() {
    for code in [
        &[0x66, 0x0f, 0x8d, 0, 0][..],
        &[0xf0, 0x7d, 0],
        &[0xf3, 0x7d, 0],
        &[0x2e, 0x7d, 0],
        &[0xe3, 0],
    ] {
        let mut image = load_pe32(&executable::pe32(code), 3).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        let before = cpu;
        let expected = if code[0] == 0xf0 {
            StopReason::InvalidInstruction
        } else {
            StopReason::UnsupportedInstruction
        };
        assert_eq!(cpu.run(&mut image.memory, 1).reason, expected);
        assert_eq!(cpu, before);
    }
}
