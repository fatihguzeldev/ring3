use ring3_core::execution::{Cpu32, GuestMemory, PAGE_SIZE, Permissions, Register32, StopReason};

// scalar x87 outputs captured with control 0x027f on x86_64 rosetta.
const CASES: &[(u64, u32, u32, f64, f64)] = &[
    (0x0000_0000_0000_0000, 0x0000_0000, 0x3f80_0000, 0.0, 1.0),
    (0x8000_0000_0000_0000, 0x8000_0000, 0x3f80_0000, -0.0, 1.0),
    (
        0x3fe0_0000_0000_0000,
        0x3ef5_7744,
        0x3f60_a940,
        0.479_425_538_604_203,
        0.877_582_561_890_372_8,
    ),
    (
        0xbfe0_0000_0000_0000,
        0xbef5_7744,
        0x3f60_a940,
        -0.479_425_538_604_203,
        0.877_582_561_890_372_8,
    ),
    (
        0x3ff9_21fb_6000_0000,
        0x3f80_0000,
        0xb33b_bd2e,
        0.999_999_999_999_999,
        -4.371_139_000_186_444e-08,
    ),
    (
        0xbff9_21fb_6000_0000,
        0xbf80_0000,
        0xb33b_bd2e,
        -0.999_999_999_999_999,
        -4.371_139_000_186_444e-08,
    ),
    (
        0x4004_0000_0000_0000,
        0x3f19_3578,
        0xbf4d_17bf,
        0.598_472_144_103_956_5,
        -0.801_143_615_546_933_7,
    ),
    (
        0xc004_0000_0000_0000,
        0xbf19_3578,
        0xbf4d_17bf,
        -0.598_472_144_103_956_5,
        -0.801_143_615_546_933_7,
    ),
    (
        0x4009_21fb_6000_0000,
        0xb3bb_bd2e,
        0xbf80_0000,
        -8.742_278_000_372_88e-08,
        -0.999_999_999_999_996_2,
    ),
    (
        0xc009_21fb_6000_0000,
        0x33bb_bd2e,
        0xbf80_0000,
        8.742_278_000_372_88e-08,
        -0.999_999_999_999_996_2,
    ),
    (
        0x4010_0000_0000_0000,
        0xbf41_bdcf,
        0xbf27_5530,
        -0.756_802_495_307_928_2,
        -0.653_643_620_863_611_9,
    ),
    (
        0xc010_0000_0000_0000,
        0x3f41_bdcf,
        0xbf27_5530,
        0.756_802_495_307_928_2,
        -0.653_643_620_863_611_9,
    ),
    (
        0x4014_0000_0000_0000,
        0xbf75_7c10,
        0x3e91_3c2c,
        -0.958_924_274_663_138_5,
        0.283_662_185_463_226_25,
    ),
    (
        0xc014_0000_0000_0000,
        0x3f75_7c10,
        0x3e91_3c2c,
        0.958_924_274_663_138_5,
        0.283_662_185_463_226_25,
    ),
    (
        0x4019_21fb_6000_0000,
        0x343b_bd2e,
        0x3f80_0000,
        1.748_455_600_074_569e-07,
        0.999_999_999_999_984_7,
    ),
    (
        0xc019_21fb_6000_0000,
        0xb43b_bd2e,
        0x3f80_0000,
        -1.748_455_600_074_569e-07,
        0.999_999_999_999_984_7,
    ),
    (
        0x4009_21fb_5100_0000,
        0x32d1_0b46,
        0xbf80_0000,
        2.433_592_895_012_851_7e-08,
        -0.999_999_999_999_999_7,
    ),
    (
        0xc009_21fb_5100_0000,
        0xb2d1_0b46,
        0xbf80_0000,
        -2.433_592_895_012_851_7e-08,
        -0.999_999_999_999_999_7,
    ),
];

fn setup(angle: f64, opcode: u8, control: u16) -> (Cpu32, GuestMemory) {
    let mut memory = GuestMemory::new(2);
    memory
        .map_zeroed(0x1000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    memory
        .write(0x1000, &[0xdd, 0x06, 0xd9, opcode, 0xdf, 0xe0, 0xdd, 0x1f])
        .unwrap();
    memory
        .protect(0x1000, PAGE_SIZE, Permissions::READ_EXECUTE)
        .unwrap();
    memory
        .map_zeroed(0x2000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    memory.write(0x2000, &angle.to_le_bytes()).unwrap();
    let mut cpu = Cpu32::new(0x1000);
    cpu.set_register(Register32::Esi, 0x2000);
    cpu.set_register(Register32::Edi, 0x2010);
    cpu.set_x87_control_word(control);
    cpu.eflags = 0xced7;
    (cpu, memory)
}

pub fn full_turn() {
    for control in [0x007f, 0x027f] {
        for &(bits, sine, cosine, sine64, cosine64) in CASES {
            let angle = f64::from_bits(bits);
            for (opcode, expected, expected64) in [(0xfe, sine, sine64), (0xff, cosine, cosine64)] {
                let (mut cpu, mut memory) = setup(angle, opcode, control);
                let result = cpu.run(&mut memory, 4);
                assert_eq!(
                    result.reason,
                    StopReason::InstructionLimit,
                    "{bits:x}/{opcode:x}/{control:x}: {result:?} {cpu:?}"
                );
                assert_eq!(cpu.eip, 0x1008);
                assert_eq!(cpu.eflags, 0xced7);
                assert_eq!(cpu.x87_control_word(), control);
                assert_eq!(
                    cpu.register(Register32::Eax) & 0x3e20,
                    0x3800 | if angle == 0.0 { 0 } else { 0x20 }
                );
                let mut output = [0; 8];
                memory.read(0x2010, &mut output).unwrap();
                let actual = f64::from_le_bytes(output);
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "oracle includes the rounded f32 result"
                )]
                let narrowed = actual as f32;
                assert_eq!(narrowed.to_bits(), expected, "{bits:x}/{opcode:x}");
                assert!(
                    (actual - expected64).abs() <= 4.0e-16,
                    "{bits:x}/{opcode:x}: {actual} vs {expected64}"
                );
            }
        }
        let outside = f64::from_bits(f64::from(std::f32::consts::TAU).to_bits() + 1);
        for angle in [outside, -outside] {
            for opcode in [0xfe, 0xff] {
                let (mut cpu, mut memory) = setup(angle, opcode, control);
                assert_eq!(cpu.run(&mut memory, 1).instructions, 1);
                let before = cpu;
                assert_eq!(
                    cpu.run(&mut memory, 1).reason,
                    StopReason::UnsupportedInstruction
                );
                assert_eq!(cpu, before);
            }
        }
    }
}
