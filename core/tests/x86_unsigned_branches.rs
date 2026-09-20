#[path = "support/executable.rs"]
mod executable;

use ring3_core::execution::{Cpu32, Register32, load_pe32};

#[test]
fn unsigned_branches_use_carry_and_preserve_flags() {
    for (encoding, below) in [
        (&[0x72, 5][..], true),
        (&[0x73, 5][..], false),
        (&[0x0f, 0x82, 5, 0, 0, 0][..], true),
        (&[0x0f, 0x83, 5, 0, 0, 0][..], false),
    ] {
        for (left, right) in [(0, 1), (1, 0), (0, 0), (u32::MAX, 1), (1, u32::MAX)] {
            let mut code = vec![0x39, 0xd8];
            code.extend_from_slice(encoding);
            code.extend_from_slice(&[0xb9, 42, 0, 0, 0, 0xcc]);
            let mut image = load_pe32(&executable::pe32(&code), 3).unwrap();
            let mut cpu = Cpu32::new(image.entry_point);
            cpu.set_register(Register32::Eax, left);
            cpu.set_register(Register32::Ebx, right);
            assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
            let flags = cpu.eflags;
            assert_eq!(cpu.run(&mut image.memory, 1).instructions, 1);
            let taken = (left < right) == below;
            assert_eq!(
                cpu.eip,
                image.entry_point
                    + 2
                    + u32::try_from(encoding.len()).unwrap()
                    + if taken { 5 } else { 0 }
            );
            assert_eq!(cpu.eflags, flags);
        }
    }
}
