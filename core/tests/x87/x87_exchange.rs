use super::executable;

use ring3_core::execution::{Cpu32, Permissions, Register32, StopReason, load_pe32};

const INPUT: u32 = 0x0040_2200;
const OUTPUT: u32 = 0x0040_2300;

fn load(code: &[u8]) -> (Cpu32, ring3_core::execution::GuestMemory) {
    let image = load_pe32(&executable::pe32(code), 32).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_x87_control_word(0x027f);
    cpu.eflags = 0xced7;
    (cpu, image.memory)
}

fn fld(code: &mut Vec<u8>, address: u32) {
    code.extend([0xdd, 0x05]);
    code.extend(address.to_le_bytes());
}

fn fstp(code: &mut Vec<u8>, address: u32) {
    code.extend([0xdd, 0x1d]);
    code.extend(address.to_le_bytes());
}

#[test]
fn fxch_exchanges_each_occupied_register_without_changing_top_or_bits() {
    for index in 0_u8..8 {
        let mut code = Vec::new();
        let mut values: Vec<u64> = (1..=8).map(|value| f64::from(value).to_bits()).collect();
        values[7] = (-0_f64).to_bits();
        for slot in 0..8_u32 {
            fld(&mut code, INPUT + slot * 8);
        }
        code.extend([0xd9, 0xc8 + index]);
        for slot in 0..8_u32 {
            fstp(&mut code, OUTPUT + slot * 8);
        }
        code.push(0xcc);
        let (mut cpu, mut memory) = load(&code);
        for (slot, value) in values.iter().enumerate() {
            memory
                .write(
                    u64::from(INPUT) + u64::try_from(slot).unwrap() * 8,
                    &value.to_le_bytes(),
                )
                .unwrap();
        }
        assert_eq!(cpu.run(&mut memory, 8).instructions, 8);
        memory
            .protect(0x0040_2000, 4096, Permissions::NONE)
            .unwrap();
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        assert_eq!(cpu.eflags, 0xced7);
        assert_eq!(cpu.x87_control_word(), 0x027f);
        memory
            .protect(0x0040_2000, 4096, Permissions::READ_WRITE)
            .unwrap();
        values.reverse();
        values.swap(0, usize::from(index));
        assert_eq!(cpu.run(&mut memory, 9).instructions, 9);
        for (slot, expected) in values.iter().enumerate() {
            let mut output = [0; 8];
            memory
                .read(
                    u64::from(OUTPUT) + u64::try_from(slot).unwrap() * 8,
                    &mut output,
                )
                .unwrap();
            assert_eq!(u64::from_le_bytes(output), *expected);
        }
    }
}

#[test]
fn fxch_rejects_empty_register_and_unsupported_prefix_without_mutation() {
    for code in [[0xd9, 0xc9].as_slice(), [0xf0, 0xd9, 0xc8].as_slice()] {
        let (mut cpu, mut memory) = load(code);
        let before = cpu;
        assert!(matches!(
            cpu.run(&mut memory, 1).reason,
            StopReason::UnsupportedInstruction | StopReason::InvalidInstruction
        ));
        assert_eq!(cpu, before);
    }

    let mut code = Vec::new();
    fld(&mut code, INPUT);
    code.extend([0xd9, 0xcb, 0xd9, 0xc8]);
    let (mut cpu, mut memory) = load(&code);
    memory
        .write(u64::from(INPUT), &1_f64.to_le_bytes())
        .unwrap();
    assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
    let before = cpu;
    assert_eq!(
        cpu.run(&mut memory, 1).reason,
        StopReason::UnsupportedInstruction
    );
    assert_eq!(cpu, before);

    cpu.eip += 2;
    cpu.set_x87_control_word(0x027e);
    let before = cpu;
    assert_eq!(
        cpu.run(&mut memory, 1).reason,
        StopReason::UnsupportedInstruction
    );
    assert_eq!(cpu, before);
}

#[test]
fn fxch_clears_c1_but_preserves_sticky_precision() {
    let mut code = Vec::new();
    fld(&mut code, INPUT);
    fld(&mut code, INPUT + 8);
    code.extend([0xd8, 0xc1, 0xd9, 0xc9, 0xdf, 0xe0, 0xcc]);
    let (mut cpu, mut memory) = load(&code);
    memory
        .write(u64::from(INPUT), &(-f64::MIN_POSITIVE).to_le_bytes())
        .unwrap();
    memory
        .write(u64::from(INPUT + 8), &1_f64.to_le_bytes())
        .unwrap();
    assert_eq!(cpu.run(&mut memory, 3).instructions, 3);
    assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
    assert_eq!(cpu.register(Register32::Eax) & 0x220, 0x20);
}
