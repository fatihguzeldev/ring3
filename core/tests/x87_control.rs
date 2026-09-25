#[path = "support/executable.rs"]
mod executable;

use ring3_core::execution::{Cpu32, MemoryError, PAGE_SIZE, Permissions, StopReason, load_pe32};

#[test]
fn control_instructions_share_state_and_touch_only_two_bytes() {
    let code = [
        0xd9, 0x3d, 0, 0x20, 0x40, 0, 0x64, 0xd9, 0x2d, 4, 0, 0, 0, 0x64, 0xd9, 0x3d, 8, 0, 0, 0,
        0xcc,
    ];
    for control in [0x007f_u16, 0x067f, 0x0b7f, 0x1f40] {
        let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
        image.memory.write(0x0040_2000, &[0xaa; 12]).unwrap();
        image
            .memory
            .write(0x0040_2004, &control.to_le_bytes())
            .unwrap();
        let mut cpu = Cpu32::new(image.entry_point);
        assert_eq!(cpu.x87_control_word(), 0x037f);
        cpu.set_fs_base(0x0040_2000);
        cpu.eflags = 0x8d7;
        assert_eq!(
            cpu.run(&mut image.memory, 10).reason,
            StopReason::Breakpoint
        );
        let mut bytes = [0; 12];
        image.memory.read(0x0040_2000, &mut bytes).unwrap();
        assert_eq!(&bytes[..4], &[0x7f, 3, 0xaa, 0xaa]);
        assert_eq!(&bytes[8..10], &control.to_le_bytes());
        assert_eq!(&bytes[10..], &[0xaa; 2]);
        assert_eq!(cpu.eflags, 0x8d7);
        assert_eq!(cpu.x87_control_word(), control);
    }
}

#[test]
fn control_memory_faults_preserve_cpu_and_do_not_partially_write() {
    for opcode in [0x2d, 0x3d] {
        for address in [0x0040_2fff_u32, u32::MAX] {
            let mut code = vec![0xd9, opcode];
            code.extend_from_slice(&address.to_le_bytes());
            let mut image = load_pe32(&executable::pe32(&code), 4).unwrap();
            image.memory.write(0x0040_2fff, &[0xaa]).unwrap();
            let mut cpu = Cpu32::new(image.entry_point);
            let before = cpu;
            let result = cpu.run(&mut image.memory, 1);
            assert!(matches!(result.reason, StopReason::MemoryFault(_)));
            if address == u32::MAX {
                assert_eq!(
                    result.reason,
                    StopReason::MemoryFault(MemoryError::AddressOverflow)
                );
            }
            assert_eq!(result.instructions, 0);
            assert_eq!(cpu, before);
            let mut byte = [0];
            image.memory.read(0x0040_2fff, &mut byte).unwrap();
            assert_eq!(byte, [0xaa]);
        }
    }
    let code = [0xd9, 0x3d, 0xff, 0x2f, 0x40, 0];
    let mut image = load_pe32(&executable::pe32(&code), 4).unwrap();
    image
        .memory
        .map_zeroed(0x0040_3000, PAGE_SIZE, Permissions::READ)
        .unwrap();
    image.memory.write(0x0040_2fff, &[0xaa]).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    let before = cpu;
    assert!(matches!(
        cpu.run(&mut image.memory, 1).reason,
        StopReason::MemoryFault(_)
    ));
    assert_eq!(cpu, before);
    let mut bytes = [0; 2];
    image.memory.read(0x0040_2fff, &mut bytes).unwrap();
    assert_eq!(bytes, [0xaa, 0]);
    image
        .memory
        .protect(0x0040_3000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    cpu.set_x87_control_word(0x027f);
    assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
    image.memory.read(0x0040_2fff, &mut bytes).unwrap();
    assert_eq!(bytes, [0x7f, 2]);
}

#[test]
fn irrational_constant_load_is_still_an_explicit_stop() {
    let mut image = load_pe32(&executable::pe32(&[0xd9, 0xe9]), 3).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    let before = cpu;
    assert_eq!(
        cpu.run(&mut image.memory, 1).reason,
        StopReason::UnsupportedInstruction
    );
    assert_eq!(cpu, before);
}
