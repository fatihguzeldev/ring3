use super::executable;

use ring3_core::execution::{Cpu32, Register32, StopReason, load_pe32};

#[test]
fn or_forms_write_results_and_update_defined_flags() {
    for (code, left, right) in [
        (&[0x0d, 0, 0, 0, 0][..], 0_u32, 0_u32),
        (&[0x81, 0xc8, 0, 0, 0, 0x80][..], 1, 0x8000_0000),
        (&[0x83, 0xc8, 0xff][..], 0, u32::MAX),
        (&[0x09, 0xd8][..], 0x1234, 0x8000_4321),
        (&[0x0b, 0xc3][..], 0xaaaa_aaaa, 0x5555_5555),
        (&[0x0b, 0x05, 0, 0x20, 0x40, 0][..], 0x8000_0000, 17),
    ] {
        let mut image = load_pe32(&executable::pe32(code), 3).unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        cpu.set_register(Register32::Eax, left);
        cpu.set_register(Register32::Ebx, right);
        cpu.eflags = 0x8d7;
        assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
        let expected = left | right;
        assert_eq!(cpu.register(Register32::Eax), expected);
        assert_eq!(cpu.eflags & 0x801, 0);
        assert_eq!(cpu.eflags & 0x40 != 0, expected == 0);
        assert_eq!(cpu.eflags & 0x80 != 0, expected >> 31 != 0);
        assert_eq!(
            cpu.eflags & 4 != 0,
            (expected & 0xff).count_ones().is_multiple_of(2)
        );
    }
}

#[test]
fn memory_or_writes_atomically_and_read_only_destination_faults() {
    let code = [0x83, 0x0d, 0, 0x20, 0x40, 0, 0xff];
    let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
    let mut value = [0; 4];
    image.memory.read(0x0040_2000, &mut value).unwrap();
    assert_eq!(u32::from_le_bytes(value), u32::MAX);
    let code = [0x09, 0x1d, 0, 0x10, 0x40, 0];
    let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_register(Register32::Ebx, u32::MAX);
    let before = cpu;
    assert!(matches!(
        cpu.run(&mut image.memory, 1).reason,
        StopReason::MemoryFault(_)
    ));
    assert_eq!(cpu, before);
    let mut unchanged = [0; 6];
    image.memory.read(0x0040_1000, &mut unchanged).unwrap();
    assert_eq!(unchanged, code);
}
