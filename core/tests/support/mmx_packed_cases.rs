use ring3_core::execution::{Cpu32, GuestMemory, PAGE_SIZE, Permissions, Register32, StopReason};

const ORACLES: &[(u8, u64)] = &[
    (0x60, 0xea7b_fede_d365_2873),
    (0x61, 0xa333_3bdd_5306_3849),
    (0x62, 0x940d_d769_5f79_0a71),
    (0x63, 0x3b9c_c3a4_5670_5041),
    (0x64, 0x57c7_e530_ddd4_6797),
    (0x65, 0xe5ca_c122_1478_d657),
    (0x66, 0x37f3_59c9_2394_cdd9),
    (0x67, 0xfe28_c735_6c0f_2820),
    (0x68, 0xd3a8_90b4_34e7_f2c3),
    (0x69, 0x0d43_8052_c431_e301),
    (0x6a, 0x9125_7a49_71af_b059),
    (0x6b, 0xf6a6_2941_0549_0e1a),
    (0x74, 0xc9d1_6f30_ab46_5d85),
    (0x75, 0xc4b4_bd11_9d65_257b),
    (0x76, 0x522e_928c_98b0_2e39),
    (0xd1, 0x813b_5e08_3fe5_0df5),
    (0xd2, 0xc0b5_f995_c3f6_9290),
    (0xd3, 0xa2f8_4196_eaf7_b3ac),
    (0xd5, 0x541c_3eb4_251e_8061),
    (0xd8, 0xb2f3_0fb9_bdee_df3e),
    (0xd9, 0xf063_f504_12cc_f4c3),
    (0xdb, 0xc4f6_92f8_12aa_20a6),
    (0xdc, 0x9300_f0b5_7b2a_26e2),
    (0xdd, 0xbde2_72ba_235d_10da),
    (0xdf, 0x2d0d_1aff_bcf5_5c4c),
    (0xe1, 0x78ba_46fa_91d9_b19f),
    (0xe2, 0x29bf_d2e3_889f_a0c9),
    (0xe5, 0xfb7c_8ea0_8a87_c826),
    (0xe8, 0x39eb_07b6_04ca_4016),
    (0xe9, 0x187d_bb04_f247_8cb1),
    (0xeb, 0xfe44_2f09_5d0e_7bc6),
    (0xec, 0xf7fc_b31b_a7a3_ad77),
    (0xed, 0x705c_5e88_5d5d_bc9b),
    (0xef, 0xb339_0f47_305d_90dd),
    (0xf1, 0x7905_41ad_5fd7_2045),
    (0xf2, 0xf438_f0a0_8b8e_d4b1),
    (0xf3, 0x8f98_a801_85bf_3f17),
    (0xf5, 0x21cd_f312_6ef2_6b3a),
    (0xf8, 0x27a5_b874_c74b_b773),
    (0xf9, 0xc4dc_fc4d_fe6c_9189),
    (0xfa, 0xa092_7092_41f8_5ea5),
    (0xfc, 0x1233_de1c_d2fb_552b),
    (0xfd, 0xcbe4_fadc_a9dd_7266),
    (0xfe, 0x28c0_e721_9108_94eb),
];

fn inputs() -> Vec<(u64, u64)> {
    let edges = [
        0,
        u64::MAX,
        0x8000_8000_8000_8000,
        0x7fff_7fff_7fff_7fff,
        0x8000_0000_8000_0000,
        0x7fff_ffff_7fff_ffff,
        0x80ff_7f00_01fe_02fd,
        0x0102_0304_0506_0708,
    ];
    let counts = [
        0,
        1,
        7,
        8,
        15,
        16,
        31,
        32,
        63,
        64,
        65,
        255,
        256,
        1 << 32,
        1 << 63,
        u64::MAX,
    ];
    let mut seed = 0x0523_3431_u64;
    (0..128)
        .map(|i| {
            if i < 64 {
                (edges[i / 8], edges[i % 8])
            } else if i < 80 {
                (edges[i % 8], counts[i - 64])
            } else {
                seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                let left = seed;
                seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                (left, seed)
            }
        })
        .collect()
}

fn setup(operation: &[u8]) -> (Cpu32, GuestMemory) {
    let mut code = vec![0x0f, 0x6f, 0x06, 0x0f, 0x6f, 0x4e, 8];
    code.extend_from_slice(operation);
    code.extend([0x0f, 0x7f, 0x07, 0x0f, 0x77]);
    let mut memory = GuestMemory::new(3);
    memory
        .map_zeroed(0x1000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    memory.write(0x1000, &code).unwrap();
    memory
        .protect(0x1000, PAGE_SIZE, Permissions::READ_EXECUTE)
        .unwrap();
    memory
        .map_zeroed(0x2000, PAGE_SIZE, Permissions::READ_WRITE)
        .unwrap();
    let mut cpu = Cpu32::new(0x1000);
    cpu.eflags = 0xced7;
    cpu.set_register(Register32::Esi, 0x2000);
    cpu.set_register(Register32::Edi, 0x2020);
    (cpu, memory)
}

fn execute(cpu: &mut Cpu32, memory: &mut GuestMemory, left: u64, right: u64) -> u64 {
    cpu.eip = 0x1000;
    memory.write(0x2000, &left.to_le_bytes()).unwrap();
    memory.write(0x2008, &right.to_le_bytes()).unwrap();
    let result = cpu.run(memory, 5);
    assert_eq!(result.reason, StopReason::InstructionLimit);
    assert_eq!(result.instructions, 5);
    assert_eq!(cpu.eflags, 0xced7);
    let mut bytes = [0; 8];
    memory.read(0x2020, &mut bytes).unwrap();
    u64::from_le_bytes(bytes)
}

pub fn packed_register_and_memory_results_match_independent_oracles() {
    let inputs = inputs();
    for &(opcode, expected) in ORACLES {
        for memory_operand in [false, true] {
            let code = if memory_operand {
                vec![0x0f, opcode, 0x46, 8]
            } else {
                vec![0x0f, opcode, 0xc1]
            };
            let (mut cpu, mut memory) = setup(&code);
            let mut hash = 0xcbf2_9ce4_8422_2325_u64;
            for &(left, right) in &inputs {
                let output = execute(&mut cpu, &mut memory, left, right);
                for byte in output.to_le_bytes() {
                    hash = (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
                }
            }
            assert_eq!(hash, expected, "opcode={opcode:x}, memory={memory_operand}");
        }
    }
}

pub fn immediate_shifts_match_verified_full_width_count_forms() {
    for (opcode, immediate_opcode, mode) in [
        (0xd1, 0x71, 0xd0),
        (0xd2, 0x72, 0xd0),
        (0xd3, 0x73, 0xd0),
        (0xe1, 0x71, 0xe0),
        (0xe2, 0x72, 0xe0),
        (0xf1, 0x71, 0xf0),
        (0xf2, 0x72, 0xf0),
        (0xf3, 0x73, 0xf0),
    ] {
        let (mut registers, mut register_memory) = setup(&[0x0f, opcode, 0xc1]);
        for (left, count) in inputs() {
            let Ok(immediate) = u8::try_from(count) else {
                continue;
            };
            let expected = execute(&mut registers, &mut register_memory, left, count);
            let (mut cpu, mut memory) = setup(&[0x0f, immediate_opcode, mode, immediate]);
            assert_eq!(execute(&mut cpu, &mut memory, left, count), expected);
        }
    }
}

pub fn narrow_unpacks_and_faults_respect_the_complete_operand_width() {
    for opcode in [0x60, 0x61, 0x62, 0x68, 0xfc] {
        let (mut cpu, mut memory) = setup(&[0x64, 0x0f, opcode, 0x06]);
        cpu.set_fs_base(0x0ffc);
        memory.write(0x2ffc, &[0x81, 0x7f, 0x80, 0xff]).unwrap();
        assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
        let before = cpu;
        let result = cpu.run(&mut memory, 1);
        if opcode <= 0x62 {
            assert_eq!(result.reason, StopReason::InstructionLimit);
        } else {
            assert!(matches!(result.reason, StopReason::MemoryFault(_)));
            assert_eq!(cpu, before);
        }
    }
    for (code, expected) in [
        ([0x66, 0x0f, 0xfc, 0xc1], StopReason::UnsupportedInstruction),
        ([0x0f, 0x38, 0x00, 0xc1], StopReason::UnsupportedInstruction),
        ([0xf3, 0x0f, 0xfc, 0xc1], StopReason::InvalidInstruction),
    ] {
        let (mut cpu, mut memory) = setup(&code);
        assert_eq!(cpu.run(&mut memory, 2).instructions, 2);
        let before = cpu;
        assert_eq!(cpu.run(&mut memory, 1).reason, expected);
        assert_eq!(cpu, before);
    }
}
