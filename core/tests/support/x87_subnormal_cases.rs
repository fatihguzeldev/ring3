use ring3_core::execution::{Cpu32, GuestMemory, Permissions, Register32, StopReason, load_pe32};

const INPUT: u32 = 0x0040_2200;
const OUTPUT: u32 = INPUT + 32;

fn operand(code: &mut Vec<u8>, opcode: u8, mode: u8, address: u32) {
    code.extend([opcode, mode]);
    code.extend(address.to_le_bytes());
}

fn fixture(code: &[u8], input: u64, control: u16) -> (Cpu32, GuestMemory) {
    let mut image = load_pe32(&super::executable::pe32(code), 16).unwrap();
    image
        .memory
        .write(u64::from(INPUT), &input.to_le_bytes())
        .unwrap();
    image.memory.write(u64::from(OUTPUT), &[0x55; 24]).unwrap();
    let mut cpu = Cpu32::new(image.entry_point);
    cpu.set_x87_control_word(control);
    (cpu, image.memory)
}

fn read<const N: usize>(memory: &GuestMemory, address: u32) -> [u8; N] {
    let mut bytes = [0; N];
    memory.read(u64::from(address), &mut bytes).unwrap();
    bytes
}

pub fn masked_subnormal_stores_match_scalar_x87() {
    // scalar x87 oracle: input, nearest bits/status, toward-zero bits/status.
    let cases = [
        (0_u64, 0_u32, 0_u32, 0_u32, 0_u32),
        (0x3690_0000_0000_0000, 0, 0x30, 0, 0x30),
        (0x36a0_0000_0000_0000, 1, 0, 1, 0),
        (0x36a8_0000_0000_0000, 2, 0x230, 1, 0x30),
        (0x36b4_0000_0000_0000, 2, 0x30, 2, 0x30),
        (0x36c2_0000_0000_0000, 4, 0x30, 4, 0x30),
        (0x380f_ffff_c000_0000, 0x7f_ffff, 0, 0x7f_ffff, 0),
        (0x380f_ffff_e000_0000, 0x80_0000, 0x230, 0x7f_ffff, 0x30),
        (0x380f_ffff_efff_ffff, 0x80_0000, 0x230, 0x7f_ffff, 0x30),
        (0x380f_ffff_f000_0000, 0x80_0000, 0x220, 0x7f_ffff, 0x30),
        (0x380f_ffff_f000_0001, 0x80_0000, 0x220, 0x7f_ffff, 0x30),
        (0x380f_ffff_ffff_ffff, 0x80_0000, 0x220, 0x7f_ffff, 0x30),
        (0x3810_0000_0000_0000, 0x80_0000, 0, 0x80_0000, 0),
        (1, 0, 0x32, 0, 0x32),
    ];
    for control in [0x007f, 0x027f, 0x0c7f] {
        for pop in [false, true] {
            for sign in [0_u64, 1 << 63] {
                for (bits, nearest, nearest_status, truncate, truncate_status) in cases {
                    let input = bits | sign;
                    let (expected, status) = if control == 0x0c7f {
                        (truncate, truncate_status)
                    } else {
                        (nearest, nearest_status)
                    };
                    let expected = expected | u32::try_from(sign >> 32).unwrap();
                    let mut code = Vec::new();
                    operand(&mut code, 0xdd, 0x05, INPUT);
                    operand(&mut code, 0xd9, if pop { 0x1d } else { 0x15 }, OUTPUT);
                    code.extend([0xdf, 0xe0]);
                    if !pop {
                        operand(&mut code, 0xdd, 0x1d, OUTPUT + 8);
                    }
                    operand(&mut code, 0xd9, 0x05, OUTPUT);
                    operand(&mut code, 0xdd, 0x1d, OUTPUT + 16);
                    code.extend([0xdf, 0xe0]);
                    let (mut cpu, mut memory) = fixture(&code, input, control);
                    assert_eq!(
                        cpu.run(&mut memory, 3).instructions,
                        3,
                        "{input:x}/{control:x}"
                    );
                    assert_eq!(u32::from_le_bytes(read(&memory, OUTPUT)), expected);
                    assert_eq!(
                        cpu.register(Register32::Eax) & 0x3a3f,
                        status | if pop { 0 } else { 0x3800 }
                    );
                    assert_eq!(
                        cpu.run(&mut memory, if pop { 3 } else { 4 }).reason,
                        StopReason::InstructionLimit
                    );
                    if !pop {
                        assert_eq!(u64::from_le_bytes(read(&memory, OUTPUT + 8)), input);
                    }
                    let promoted = f64::from(f32::from_bits(expected));
                    assert_eq!(
                        u64::from_le_bytes(read(&memory, OUTPUT + 16)),
                        promoted.to_bits()
                    );
                    assert_eq!(
                        cpu.register(Register32::Eax) & 0x3a3f,
                        (status & !0x200)
                            | if f32::from_bits(expected).is_subnormal() {
                                2
                            } else {
                                0
                            }
                    );
                }
            }
        }
    }
}

pub fn subnormal_faults_and_memory_operands_preserve_state() {
    let mut refused = vec![0xd9, 0xe8];
    operand(&mut refused, 0xd8, 0x35, INPUT);
    let (mut cpu, mut memory) = fixture(&refused, 1, 0x007f);
    assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
    let before = cpu;
    assert_eq!(
        cpu.run(&mut memory, 1).reason,
        StopReason::UnsupportedInstruction
    );
    assert_eq!(cpu, before);
    for pop in [false, true] {
        let mut code = Vec::new();
        operand(&mut code, 0xdd, 0x05, INPUT);
        operand(&mut code, 0xd9, if pop { 0x1d } else { 0x15 }, OUTPUT);
        let (mut cpu, mut memory) = fixture(&code, 0xb6c2_0000_0000_0000, 0x027f);
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        memory
            .protect(0x0040_2000, 4096, Permissions::READ)
            .unwrap();
        let before = cpu;
        assert!(matches!(
            cpu.run(&mut memory, 1).reason,
            StopReason::MemoryFault(_)
        ));
        assert_eq!(cpu, before);
        assert_eq!(read::<4>(&memory, OUTPUT), [0x55; 4]);
        memory
            .protect(0x0040_2000, 4096, Permissions::READ_WRITE)
            .unwrap();
        assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
        assert_eq!(u32::from_le_bytes(read(&memory, OUTPUT)), 0x8000_0004);
    }
    for bits in [1_u32, 0x7f_ffff, 0x8000_0001, 0x807f_ffff] {
        // multiplication and comparison must report a denormal memory operand.
        for compare in [false, true] {
            let mut code = vec![0xd9, 0xe8];
            operand(&mut code, 0xd8, if compare { 0x15 } else { 0x0d }, INPUT);
            code.extend([0xdf, 0xe0]);
            operand(&mut code, 0xdd, 0x1d, OUTPUT);
            let (mut cpu, mut memory) = fixture(&code, u64::from(bits), 0x027f);
            assert_eq!(cpu.run(&mut memory, 4).instructions, 4);
            assert_eq!(cpu.register(Register32::Eax) & 0x473f, 2);
            let expected = if compare {
                1.0
            } else {
                f64::from(f32::from_bits(bits))
            };
            assert_eq!(
                u64::from_le_bytes(read(&memory, OUTPUT)),
                expected.to_bits()
            );
        }
    }
}
