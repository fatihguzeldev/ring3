use ring3_core::execution::{Cpu32, Register32};

pub fn assert_unchanged(cpu: &Cpu32, expected: &Cpu32) {
    for register in [
        Register32::Eax,
        Register32::Ecx,
        Register32::Edx,
        Register32::Ebx,
        Register32::Esp,
        Register32::Ebp,
        Register32::Esi,
        Register32::Edi,
    ] {
        assert_eq!(cpu.register(register), expected.register(register));
    }
    assert_eq!(cpu.eip, expected.eip);
    assert_eq!(cpu.eflags, expected.eflags);
    assert_eq!(cpu.fs_base(), expected.fs_base());
    assert_eq!(cpu.x87_control_word(), expected.x87_control_word());
}
