#[path = "support/executable.rs"]
mod executable;
#[path = "support/fpu_wait_executable.rs"]
mod fpu_wait_executable;

use ring3_core::execution::{Cpu32, StopReason, load_pe32};

#[test]
fn wait_and_control_stores_remain_separate_whole_or_stepwise_instructions() {
    for budget in [1, 20] {
        let mut image = load_pe32(&fpu_wait_executable::pe32(), 4).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        let mut count = 0;
        loop {
            let run = cpu.run(&mut image.memory, budget);
            count += run.instructions;
            if run.reason != StopReason::InstructionLimit {
                assert_eq!(run.reason, StopReason::Breakpoint);
                break;
            }
            assert!(count < 20);
        }
        assert_eq!(count, 6);
        assert_eq!(cpu.x87_control_word(), 0x0f7f);
        let mut bytes = [0; 4];
        image.memory.read(0x0040_2180, &mut bytes).unwrap();
        assert_eq!(bytes, [0x7f, 3, 0xaa, 0xaa]);
    }
}

#[test]
fn masked_wait_preserves_live_stack_flags_control_and_memory() {
    let code = [0xdd, 0x05, 0x80, 0x21, 0x40, 0, 0x9b];
    let mut image = load_pe32(&executable::pe32(&code), 4).unwrap();
    image
        .memory
        .write(0x0040_2180, &1.75_f64.to_le_bytes())
        .unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_x87_control_word(0x027f);
    cpu.eflags = 0xced7;
    assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
    let wait = cpu.eip;
    let mut original = vec![0; 4096];
    image.memory.read(0x0040_2000, &mut original).unwrap();
    for control in [0x003f, 0x007f, 0x027f, 0x037f, 0x0f7f, 0xffff] {
        cpu.eip = wait;
        cpu.set_x87_control_word(control);
        let mut expected = cpu;
        assert_eq!(cpu.run(&mut image.memory, 0).instructions, 0);
        assert_eq!(cpu, expected);
        let run = cpu.run(&mut image.memory, 1);
        assert_eq!(
            (run.reason, run.instructions),
            (StopReason::InstructionLimit, 1)
        );
        expected.eip += 1;
        assert_eq!(cpu, expected);
        let mut bytes = vec![0; 4096];
        image.memory.read(0x0040_2000, &mut bytes).unwrap();
        assert_eq!(bytes, original);
    }
    for bit in 0..6 {
        cpu.eip = wait;
        cpu.set_x87_control_word(0x027f & !(1 << bit));
        let before = cpu;
        let run = cpu.run(&mut image.memory, 1);
        assert_eq!(
            (run.reason, run.instructions),
            (StopReason::UnsupportedInstruction, 0)
        );
        assert_eq!(cpu, before);
    }
}

#[test]
fn following_control_fault_and_unsupported_instruction_keep_their_own_eip() {
    for following in [&[0xd9, 0x3d, 0xff, 0x2f, 0x40, 0][..], &[0xdb, 0xe3][..]] {
        let mut code = vec![0x9b];
        code.extend_from_slice(following);
        let mut image = load_pe32(&executable::pe32(&code), 4).unwrap();
        image.memory.write(0x0040_2fff, &[0xaa]).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        let mut expected = cpu;
        expected.eip += 1;
        let run = cpu.run(&mut image.memory, 10);
        assert_eq!(run.instructions, 1);
        if following[0] == 0xd9 {
            assert!(matches!(run.reason, StopReason::MemoryFault(_)));
        } else {
            assert_eq!(run.reason, StopReason::UnsupportedInstruction);
        }
        assert_eq!(cpu, expected);
        let mut byte = [0];
        image.memory.read(0x0040_2fff, &mut byte).unwrap();
        assert_eq!(byte, [0xaa]);
    }
    let mut image = load_pe32(&executable::pe32(&[0xf0, 0x9b]), 4).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    let before = cpu;
    assert_eq!(
        cpu.run(&mut image.memory, 1).reason,
        StopReason::InvalidInstruction
    );
    assert_eq!(cpu, before);
}
