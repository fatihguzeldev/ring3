#[path = "support/executable.rs"]
mod executable;

use ring3_core::execution::{Cpu32, Register32, StopReason, load_pe32};

#[test]
fn cdq_extends_the_eax_sign_without_changing_other_cpu_state() {
    for eax in [0, 1, 0x7fff_ffff, 0x8000_0000, u32::MAX] {
        let mut image = load_pe32(&executable::pe32(&[0x99]), 3).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        cpu.set_register(Register32::Eax, eax);
        cpu.set_register(Register32::Ecx, 0x1234_5678);
        cpu.set_register(Register32::Edx, 0xa5a5_a5a5);
        cpu.eflags = 0xced7;
        let mut expected = cpu;
        expected.eip += 1;
        expected.set_register(
            Register32::Edx,
            if eax & 0x8000_0000 == 0 { 0 } else { u32::MAX },
        );

        let result = cpu.run(&mut image.memory, 1);
        assert_eq!(result.reason, StopReason::InstructionLimit);
        assert_eq!(result.instructions, 1);
        assert_eq!(cpu, expected, "eax={eax:#x}");
    }
}

#[test]
fn zero_budget_and_unsupported_cwd_preserve_the_complete_cpu() {
    let mut image = load_pe32(&executable::pe32(&[0x99]), 3).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_register(Register32::Eax, 0x8000_0000);
    cpu.set_register(Register32::Edx, 7);
    cpu.eflags = 0xced7;
    let before = cpu;
    assert_eq!(
        cpu.run(&mut image.memory, 0).reason,
        StopReason::InstructionLimit
    );
    assert_eq!(cpu, before);

    let mut image = load_pe32(&executable::pe32(&[0x66, 0x99]), 3).unwrap();
    let mut cpu = before;
    cpu.eip = image.entry_point;
    let before = cpu;
    assert_eq!(
        cpu.run(&mut image.memory, 1).reason,
        StopReason::UnsupportedInstruction
    );
    assert_eq!(cpu, before);
}
